//! Agent coordinator for managing multi-agent teams

use crate::{
    error::{AgentError, CoordinationError},
    message::{AgentMessage, MessageContent, MessageType, ProtocolMessage},
    persistence::{FilePersistence, InboxMessage, TeamConfigFile},
    process_manager::{AgentProcessConfig, AgentProcessManager, AgentEvent},
    task::{AgentTask, TaskPriority, TaskStatus},
    teammate::{Teammate, TeammateConfig, TeammateStatus},
    worktree::{WorktreeConfig, WorktreeManager},
    TaskBoard,
};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use shannon_core::hooks::{HookEvent, HookManager};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, broadcast, watch};
use uuid::Uuid;

/// Information about a team member for agent discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    /// Agent name
    pub name: String,
    /// Agent type/role
    pub agent_type: String,
    /// Agent capabilities
    pub capabilities: Vec<String>,
}

/// Manifest of a team's members for agent discovery.
/// Can be injected into spawned agents' system prompts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamManifest {
    /// Team name
    pub name: String,
    /// Team description
    pub description: String,
    /// Team members
    pub members: Vec<AgentInfo>,
}

/// Summary of an agent's inbox across all teams.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InboxSummary {
    /// Total messages in inbox
    pub total: usize,
    /// Unread message count
    pub unread: usize,
    /// Unique senders who have messaged this agent
    pub senders: Vec<String>,
}

/// Configuration for the agent coordinator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// Maximum number of agents in a team
    pub max_team_size: usize,
    /// Channel buffer size for agent messages
    pub message_buffer_size: usize,
    /// Enable worktree isolation for agents
    pub enable_worktree_isolation: bool,
    /// Worktree configuration (if enabled)
    pub worktree_config: Option<WorktreeConfig>,
    /// Heartbeat interval in seconds
    pub heartbeat_interval_secs: u64,
    /// Task assignment strategy
    pub assignment_strategy: AssignmentStrategy,
    /// Delegate mode: when true, the lead agent only coordinates (creates tasks,
    /// messages teammates) and does not directly implement code changes.
    /// Inspired by Claude Code's Shift+Tab delegate mode.
    pub delegate_mode: bool,
    /// Agent execution mode: in-process (default) or separate OS process.
    #[serde(default)]
    pub agent_mode: AgentMode,
}

/// Strategy for assigning tasks to agents
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssignmentStrategy {
    /// Round-robin assignment
    RoundRobin,
    /// Assign to least loaded agent
    LeastLoaded,
    /// Assign based on agent capabilities
    CapabilityBased,
    /// First available agent
    FirstAvailable,
    /// Agents self-claim the lowest-ID unblocked task (Claude Code approach)
    SelfClaim,
}

/// Agent execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentMode {
    /// Agents run as in-process tokio tasks (default).
    #[default]
    InProcess,
    /// Agents run as separate OS processes with JSON-RPC IPC.
    Process,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            max_team_size: 10,
            message_buffer_size: 100,
            enable_worktree_isolation: false,
            worktree_config: None,
            heartbeat_interval_secs: 30,
            assignment_strategy: AssignmentStrategy::SelfClaim,
            delegate_mode: false,
            agent_mode: AgentMode::default(),
        }
    }
}

/// Event from the coordinator
#[derive(Debug, Clone)]
pub enum CoordinatorEvent {
    /// Agent joined the team
    AgentJoined { team: String, agent: String },
    /// Agent left the team
    AgentLeft { team: String, agent: String },
    /// Message sent between agents
    MessageSent(AgentMessage),
    /// Task assigned to agent
    TaskAssigned { task_id: Uuid, agent: String },
    /// Task completed
    TaskCompleted { task_id: Uuid, agent: String },
    /// Task failed
    TaskFailed { task_id: Uuid, agent: String, reason: String },
    /// Agent status changed
    StatusChanged { agent: String, status: TeammateStatus },
    /// Team was disbanded/deleted
    TeamDeleted { team: String },
    /// Background agent task produced output (streaming chunk)
    AgentOutput { team: String, agent: String, chunk: String },
    /// Background agent task completed
    AgentCompleted { team: String, agent: String, success: bool, output: String },
    /// An idle agent was auto-assigned a task
    TaskAutoClaimed { task_id: Uuid, team: String, agent: String },
}

/// Main coordinator for managing multi-agent teams
#[allow(dead_code)]
pub struct AgentCoordinator {
    config: CoordinatorConfig,
    teams: Arc<RwLock<HashMap<String, AgentTeam>>>,
    worktree_manager: Option<WorktreeManager>,
    task_board: Arc<TaskBoard>,
    message_sender: mpsc::Sender<AgentMessage>,
    event_sender: broadcast::Sender<CoordinatorEvent>,
    _message_receiver: Arc<tokio::task::JoinHandle<()>>,
    _heartbeat_handle: Arc<tokio::task::JoinHandle<()>>,
    /// Optional file-based persistence layer
    persistence: Option<FilePersistence>,
    /// Active background tasks keyed by task ID for cancellation
    background_tasks: Arc<RwLock<HashMap<String, tokio::task::AbortHandle>>>,
    /// Runtime delegate mode flag (toggled after construction)
    delegate_mode_flag: std::sync::atomic::AtomicBool,
    /// Background task that auto-delivers inbox messages to teammates
    _delivery_handle: Option<tokio::task::JoinHandle<()>>,
    /// Sender side of the cancellation signal for the delivery loop.
    /// Sending `true` tells the loop to stop.
    delivery_cancel: watch::Sender<bool>,
    /// Optional hook manager for firing team-related hooks
    hook_manager: Option<Arc<HookManager>>,
    /// Process manager for out-of-process agents (when agent_mode == Process).
    /// The event receiver is consumed by a background forwarding task.
    process_manager: Option<AgentProcessManager>,
    /// Channel for RPC requests from process agents that need coordinator state.
    rpc_request_rx: Option<mpsc::Receiver<(String, i64, String, serde_json::Value)>>,
}

/// A team of agents working together
#[derive(Debug, Clone)]
struct AgentTeam {
    name: String,
    description: String,
    members: HashMap<String, Teammate>,
    task_list: Vec<AgentTask>,
    created_at: chrono::DateTime<chrono::Utc>,
    /// Current round-robin index for task assignment
    assignment_index: usize,
}

impl AgentTeam {
    /// Return a human-readable summary of this team.
    fn summary(&self) -> String {
        format!(
            "Team '{}' ({} members, {} tasks) — {} [created {}]",
            self.name,
            self.members.len(),
            self.task_list.len(),
            self.description,
            self.created_at.format("%Y-%m-%d %H:%M UTC"),
        )
    }
}

impl AgentCoordinator {
    /// Create a new agent coordinator
    pub async fn new(config: CoordinatorConfig) -> Result<Self, AgentError> {
        let worktree_manager = if config.enable_worktree_isolation {
            Some(
                WorktreeManager::new(
                    config.worktree_config.clone().unwrap_or_default(),
                )
                .await?,
            )
        } else {
            None
        };

        let task_board = Arc::new(TaskBoard::new());
        let teams = Arc::new(RwLock::new(HashMap::new()));
        let (message_sender, mut message_receiver) = mpsc::channel(config.message_buffer_size);
        let (event_sender, _) = broadcast::channel(100);

        // Clone for the message handler task
        let teams_clone = teams.clone();
        let task_board_clone = task_board.clone();
        let event_sender_clone = event_sender.clone();

        // Spawn message handling task
        let message_handle = tokio::spawn(async move {
            while let Some(message) = message_receiver.recv().await {
                if let Err(e) = Self::handle_message_internal(
                    message,
                    &teams_clone,
                    &task_board_clone,
                    &event_sender_clone,
                ).await {
                    tracing::error!("Error handling message: {}", e);
                }
            }
        });

        // Spawn heartbeat task
        let teams_heartbeat = teams.clone();
        let event_heartbeat = event_sender.clone();
        let task_board_heartbeat = task_board.clone();
        let heartbeat_interval = config.heartbeat_interval_secs;
        let assignment_strategy = config.assignment_strategy;
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(heartbeat_interval));
            // Consume the first immediate tick so heartbeat waits a full interval
            // before firing. This avoids racing with test setup and initial task creation.
            interval.tick().await;
            loop {
                interval.tick().await;
                Self::send_heartbeats(&teams_heartbeat, &event_heartbeat).await;
                if matches!(assignment_strategy, AssignmentStrategy::SelfClaim) {
                    Self::auto_claim_idle_agents(
                        &teams_heartbeat, &task_board_heartbeat, &event_heartbeat,
                    ).await;
                }
            }
        });

        let delegate_mode = config.delegate_mode;
        let agent_mode = config.agent_mode;

        // Cancellation channel for the inbox delivery loop
        let (delivery_cancel, mut delivery_cancel_rx) = watch::channel(false);

        // Clone event_sender before moving into struct (needed for process manager forwarding)
        let event_sender_for_pm = event_sender.clone();

        let mut coordinator = Self {
            config,
            teams,
            worktree_manager,
            task_board,
            message_sender,
            event_sender,
            _message_receiver: Arc::new(message_handle),
            _heartbeat_handle: Arc::new(heartbeat_handle),
            persistence: None,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            delegate_mode_flag: std::sync::atomic::AtomicBool::new(delegate_mode),
            _delivery_handle: None,
            delivery_cancel,
            hook_manager: None,
            process_manager: None,
            rpc_request_rx: None,
        };

        // Start the background inbox delivery loop
        coordinator.start_inbox_delivery_loop(&mut delivery_cancel_rx);

        // If process mode is enabled, create the process manager and event forwarder
        if agent_mode == AgentMode::Process {
            let mut pm = AgentProcessManager::new();
            let event_rx = pm.take_event_receiver();

            // Channel for RPC requests that need coordinator state
            let (rpc_tx, rpc_rx) = mpsc::channel::<(String, i64, String, serde_json::Value)>(64);
            coordinator.rpc_request_rx = Some(rpc_rx);

            // Spawn event forwarding task: translates AgentEvent → CoordinatorEvent
            tokio::spawn(async move {
                let mut rx = event_rx;
                while let Some(agent_event) = rx.recv().await {
                    let coord_event = match agent_event {
                        AgentEvent::Ready { agent_name, capabilities: _ } => {
                            tracing::info!(agent = %agent_name, "Process agent ready");
                            None
                        }
                        AgentEvent::Progress { agent_name, task_id, chunk } => {
                            Some(CoordinatorEvent::AgentOutput {
                                team: String::new(), // filled by subscriber
                                agent: agent_name,
                                chunk: format!("[{}] {}", task_id, chunk),
                            })
                        }
                        AgentEvent::TaskComplete { agent_name, task_id, success, output } => {
                            if success {
                                Some(CoordinatorEvent::TaskCompleted {
                                    task_id: Uuid::parse_str(&task_id).unwrap_or(Uuid::nil()),
                                    agent: agent_name,
                                })
                            } else {
                                Some(CoordinatorEvent::TaskFailed {
                                    task_id: Uuid::parse_str(&task_id).unwrap_or(Uuid::nil()),
                                    agent: agent_name,
                                    reason: output,
                                })
                            }
                        }
                        AgentEvent::Idle { agent_name, available_tasks_count: _ } => {
                            Some(CoordinatorEvent::StatusChanged {
                                agent: agent_name,
                                status: TeammateStatus::Idle,
                            })
                        }
                        AgentEvent::ProcessExited { agent_name, exit_code } => {
                            tracing::warn!(
                                agent = %agent_name,
                                exit_code = ?exit_code,
                                "Process agent exited"
                            );
                            Some(CoordinatorEvent::StatusChanged {
                                agent: agent_name,
                                status: TeammateStatus::Stopped,
                            })
                        }
                        AgentEvent::HealthCheckFailed { agent_name, consecutive_failures } => {
                            tracing::warn!(
                                agent = %agent_name,
                                failures = consecutive_failures,
                                "Process agent health check failed"
                            );
                            None
                        }
                        AgentEvent::AgentRestarted { agent_name, restart_count } => {
                            tracing::info!(
                                agent = %agent_name,
                                restart_count,
                                "Process agent restarted"
                            );
                            Some(CoordinatorEvent::StatusChanged {
                                agent: agent_name,
                                status: TeammateStatus::Idle,
                            })
                        }
                        AgentEvent::RpcRequest { agent_name, request_id, method, params } => {
                            tracing::debug!(
                                agent = %agent_name,
                                method = %method,
                                request_id,
                                "Forwarding RPC request from agent"
                            );
                            let _ = rpc_tx.send((agent_name, request_id, method, params)).await;
                            None
                        }
                    };
                    if let Some(event) = coord_event {
                        let _ = event_sender_for_pm.send(event);
                    }
                }
            });

            coordinator.process_manager = Some(pm);
        }

        Ok(coordinator)
    }

    /// Create a new agent team
    pub async fn create_team(
        &self,
        name: String,
        description: String,
    ) -> Result<(), AgentError> {
        let mut teams = self.teams.write().await;

        if teams.contains_key(&name) {
            return Err(AgentError::Coordination(
                CoordinationError::InvalidConfiguration(format!("team '{name}' already exists"))
            ));
        }

        let team = AgentTeam {
            name: name.clone(),
            description,
            members: HashMap::new(),
            task_list: Vec::new(),
            created_at: chrono::Utc::now(),
            assignment_index: 0,
        };

        teams.insert(name.clone(), team);
        drop(teams);

        // Persist to disk if persistence layer is configured
        self.persist_team(&name).await;

        tracing::info!(team = %name, "Team created");

        Ok(())
    }

    /// Add a teammate to a team
    pub async fn add_teammate(
        &self,
        team_name: &str,
        agent_name: String,
        config: TeammateConfig,
    ) -> Result<(), AgentError> {
        let mut teams = self.teams.write().await;

        let team = teams.get_mut(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        if team.members.len() >= self.config.max_team_size {
            return Err(AgentError::Coordination(
                CoordinationError::MaxTeamSizeExceeded(self.config.max_team_size)
            ));
        }

        if team.members.contains_key(&agent_name) {
            return Err(AgentError::Coordination(
                CoordinationError::AgentAlreadyMember(agent_name, team_name.to_string())
            ));
        }

        // Inject team manifest into agent's system prompt so it knows its teammates
        let mut config = config;
        let manifest = self.team_manifest_internal(&team).await;
        let manifest_suffix = format!(
            "\n\n## Your Team: {}\n{}",
            manifest.name,
            manifest.members.iter()
                .filter(|m| m.name != agent_name)
                .map(|m| format!("- {} ({}): {}", m.name, m.agent_type, m.capabilities.join(", ")))
                .collect::<Vec<_>>()
                .join("\n")
        );
        config.system_prompt = Some(match config.system_prompt {
            Some(sp) => format!("{sp}{manifest_suffix}"),
            None => format!("You are a team agent.{manifest_suffix}"),
        });

        // Clone fields needed for process spawning before config is moved
        let config_model = config.model.clone();
        let config_system_prompt = config.system_prompt.clone();
        let config_allowed_tools = if config.allowed_tools.is_empty() {
            None
        } else {
            Some(config.allowed_tools.clone())
        };

        let teammate = Teammate::new(agent_name.clone(), config);
        team.members.insert(agent_name.clone(), teammate);

        // Create isolated worktree for this agent if worktree isolation is enabled
        if self.config.enable_worktree_isolation {
            if let Some(ref wm) = self.worktree_manager {
                match wm.create_agent_session(&agent_name, None).await {
                    Ok(session) => {
                        tracing::info!(
                            team = %team_name,
                            agent = %agent_name,
                            worktree = %session.path.display(),
                            "Created isolated worktree for agent"
                        );
                        // Store worktree path in teammate metadata
                        let agent = team.members.get(&agent_name).unwrap();
                        agent.set_metadata("worktree_path".to_string(),
                            serde_json::json!(session.path.to_string_lossy().to_string())).await;
                        agent.set_metadata("worktree_branch".to_string(),
                            serde_json::json!(session.branch_name)).await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            team = %team_name,
                            agent = %agent_name,
                            error = %e,
                            "Failed to create worktree for agent, continuing without isolation"
                        );
                    }
                }
            }
        }

        drop(teams);

        // If in process mode, spawn an OS process for this agent
        if self.config.agent_mode == AgentMode::Process {
            if let Some(ref pm) = self.process_manager {
                // Determine binary path: prefer the current executable (shannon CLI)
                let binary_path = std::env::current_exe()
                    .unwrap_or_else(|_| PathBuf::from("shannon"));

                // Get worktree path if isolation is enabled
                let worktree_path = {
                    let teams = self.teams.read().await;
                    match teams.get(team_name).and_then(|t| t.members.get(&agent_name)) {
                        Some(agent) => agent.get_metadata("worktree_path").await
                            .and_then(|v| v.as_str().map(|s| s.to_string()))
                            .map(PathBuf::from),
                        None => None,
                    }
                };

                let process_config = AgentProcessConfig {
                    binary_path,
                    args: vec!["--team-agent".to_string()],
                    env: HashMap::new(),
                    worktree_path,
                    model: config_model,
                    system_prompt: config_system_prompt,
                    agent_name: agent_name.clone(),
                    permission_mode: Some("bypassPermissions".to_string()),
                    allowed_tools: config_allowed_tools,
                };

                match pm.spawn_agent(process_config).await {
                    Ok(_) => {
                        tracing::info!(
                            team = %team_name,
                            agent = %agent_name,
                            "Spawned process agent"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            team = %team_name,
                            agent = %agent_name,
                            error = %e,
                            "Failed to spawn process agent"
                        );
                    }
                }
            }
        }

        // Persist updated team config to disk
        self.persist_team(team_name).await;

        if let Err(e) = self.event_sender.send(CoordinatorEvent::AgentJoined {
            team: team_name.to_string(),
            agent: agent_name.clone(),
        }) {
            tracing::warn!(
                team = %team_name,
                agent = %agent_name,
                error = %e,
                "Failed to send AgentJoined event - no active receivers"
            );
        }

        tracing::info!(
            team = %team_name,
            agent = %agent_name,
            "Teammate added to team"
        );

        Ok(())
    }

    /// Send a message to an agent or broadcast to all
    pub async fn send_message(&self, message: AgentMessage) -> Result<(), AgentError> {
        self.message_sender.send(message).await
            .map_err(|e| AgentError::Communication(format!("Failed to send message: {e}")))?;

        Ok(())
    }

    // ── Peer-to-Peer + Team-Scoped Messaging ──────────────────────────

    /// Send a direct P2P message from one agent to another within a team.
    ///
    /// Routes the message through the team's members, delivers to the
    /// recipient's inbox, and persists it if persistence is configured.
    pub async fn send_direct_message(
        &self,
        team_name: &str,
        from: &str,
        to: &str,
        content: MessageContent,
    ) -> Result<AgentMessage, AgentError> {
        let message = AgentMessage {
            id: Uuid::new_v4(),
            from: from.to_string(),
            to: to.to_string(),
            message_type: MessageType::Chat,
            priority: crate::message::MessagePriority::Normal,
            content,
            timestamp: chrono::Utc::now(),
        };

        // Deliver to the recipient agent within the team
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let agent = team.members.get(to)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::AgentNotFound(to.to_string())
            ))?;

        let response = agent.handle_message(message.clone()).await?;

        // Persist to inbox if persistence is configured
        if let Some(ref persist) = self.persistence {
            let inbox_msg = InboxMessage {
                id: message.id.to_string(),
                from: message.from.clone(),
                content: format!("{:?}", message.content),
                timestamp: message.timestamp.to_rfc3339(),
                read: false,
            };
            if let Err(e) = persist.deliver_message(team_name, to, &inbox_msg) {
                tracing::warn!(agent = %to, error = %e, "Failed to persist inbox message");
            }
        }

        if let Err(e) = self.event_sender.send(CoordinatorEvent::MessageSent(message.clone())) {
            tracing::warn!(error = %e, "Failed to send MessageSent event");
        }

        Ok(response)
    }

    /// Broadcast a message to all members of a specific team.
    ///
    /// Unlike the global `"*"` broadcast which hits all teams,
    /// this sends only to members of the named team.
    pub async fn broadcast_to_team(
        &self,
        team_name: &str,
        from: &str,
        content: String,
    ) -> Result<Vec<AgentMessage>, AgentError> {
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let mut responses = Vec::new();

        for (agent_name, agent) in &team.members {
            if agent_name == from {
                continue; // Don't send to self
            }

            let message = AgentMessage {
                id: Uuid::new_v4(),
                from: from.to_string(),
                to: agent_name.clone(),
                message_type: MessageType::Chat,
                priority: crate::message::MessagePriority::Normal,
                content: MessageContent::Text(content.clone()),
                timestamp: chrono::Utc::now(),
            };

            match agent.handle_message(message.clone()).await {
                Ok(response) => {
                    // Persist to inbox
                    if let Some(ref persist) = self.persistence {
                        let inbox_msg = InboxMessage {
                            id: message.id.to_string(),
                            from: message.from.clone(),
                            content: content.clone(),
                            timestamp: message.timestamp.to_rfc3339(),
                            read: false,
                        };
                        if let Err(e) = persist.deliver_message(team_name, agent_name, &inbox_msg) {
                            tracing::warn!(agent = %agent_name, error = %e, "Failed to persist inbox message");
                        }
                    }

                    if let Err(e) = self.event_sender.send(CoordinatorEvent::MessageSent(message)) {
                        tracing::warn!(error = %e, "Failed to send MessageSent event");
                    }

                    responses.push(response);
                }
                Err(e) => {
                    tracing::warn!(
                        from = %from,
                        to = %agent_name,
                        error = %e,
                        "Failed to deliver broadcast message"
                    );
                }
            }
        }

        tracing::info!(
            from = %from,
            team = %team_name,
            recipients = responses.len(),
            "Team broadcast sent"
        );

        Ok(responses)
    }

    /// Read unread messages from an agent's persisted inbox.
    /// Searches across all teams for the agent's inbox.
    pub async fn read_agent_inbox(&self, agent_name: &str) -> Vec<InboxMessage> {
        if let Some(ref persist) = self.persistence {
            // Search all teams for this agent
            let team_names = self.list_teams().await;
            for team_name in &team_names {
                match persist.read_inbox(team_name, agent_name) {
                    Ok(messages) if !messages.is_empty() => return messages,
                    Ok(_) => continue,
                    Err(e) => {
                        tracing::warn!(agent = %agent_name, error = %e, "Failed to read inbox");
                    }
                }
            }
        }
        Vec::new()
    }

    /// Get a summary of an agent's inbox across all teams.
    ///
    /// Returns the total message count, unread count, and a list of unique senders.
    /// Useful for agents to quickly check their inbox status without loading full messages.
    pub async fn inbox_summary(&self, agent_name: &str) -> InboxSummary {
        let mut summary = InboxSummary::default();
        let team_names = self.list_teams().await;
        if let Some(ref persist) = self.persistence {
            for team_name in &team_names {
                if let Ok(messages) = persist.read_inbox(team_name, agent_name) {
                    for msg in &messages {
                        summary.total += 1;
                        if !msg.read {
                            summary.unread += 1;
                        }
                        if !summary.senders.contains(&msg.from) {
                            summary.senders.push(msg.from.clone());
                        }
                    }
                }
            }
        }
        summary
    }

    /// Subscribe to coordinator events
    pub fn subscribe_events(&self) -> broadcast::Receiver<CoordinatorEvent> {
        self.event_sender.subscribe()
    }

    /// Get a list of all teams
    pub async fn list_teams(&self) -> Vec<String> {
        self.teams.read().await.keys().cloned().collect()
    }

    /// Get team members
    pub async fn get_team_members(&self, team_name: &str) -> Result<Vec<String>, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        Ok(team.members.keys().cloned().collect())
    }

    /// Get agent by name
    pub async fn get_agent(&self, team_name: &str, agent_name: &str) -> Result<Teammate, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        team.members.get(agent_name)
            .cloned()
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::AgentNotFound(agent_name.to_string())
            ))
    }

    /// Add a task to the team
    pub async fn add_task(
        &self,
        team_name: &str,
        subject: String,
        description: String,
        priority: TaskPriority,
    ) -> Result<Uuid, AgentError> {
        let task = AgentTask::new(subject.clone(), description, priority);
        let task_id = task.id;

        self.task_board.add_task(task).await?;

        let mut teams = self.teams.write().await;
        if let Some(team) = teams.get_mut(team_name) {
            team.task_list.push(AgentTask {
                id: task_id,
                subject: String::new(), // Placeholder
                description: String::new(),
                status: TaskStatus::Pending,
                priority: TaskPriority::Medium,
                owner: None,
                blocked_by: Vec::new(),
                blocks: Vec::new(),
                active_form: None,
                required_capabilities: Vec::new(),
                metadata: serde_json::Value::Null,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }

        // Fire TeamTaskCreated hook
        self.fire_hook(HookEvent::TeamTaskCreated {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: None,
            subject: subject.clone(),
            priority: format!("{:?}", priority),
        });

        Ok(task_id)
    }

    /// Add a task to the team with explicit dependency declarations.
    ///
    /// Creates the task and registers `blocked_by` dependencies on the task board.
    /// Each dependency ID in `blocked_by` must correspond to an existing task that
    /// must complete before this new task can be claimed by an agent.
    pub async fn add_task_with_deps(
        &self,
        team_name: &str,
        subject: String,
        description: String,
        priority: TaskPriority,
        blocked_by: Vec<Uuid>,
    ) -> Result<Uuid, AgentError> {
        let mut task = AgentTask::new(subject.clone(), description, priority);
        let task_id = task.id;

        // Register dependencies on the task itself before adding to the board
        for dep_id in &blocked_by {
            task.add_dependency(*dep_id);
        }

        self.task_board.add_task(task).await?;

        // Register dependencies on the task board for graph tracking
        for dep_id in &blocked_by {
            if let Err(e) = self.task_board.add_dependency(task_id, *dep_id).await {
                tracing::warn!(
                    task_id = %task_id,
                    dep_id = %dep_id,
                    error = %e,
                    "Failed to register task dependency"
                );
            }
        }

        let mut teams = self.teams.write().await;
        if let Some(team) = teams.get_mut(team_name) {
            team.task_list.push(AgentTask {
                id: task_id,
                subject: String::new(),
                description: String::new(),
                status: TaskStatus::Pending,
                priority: TaskPriority::Medium,
                owner: None,
                blocked_by: blocked_by.clone(),
                blocks: Vec::new(),
                active_form: None,
                required_capabilities: Vec::new(),
                metadata: serde_json::Value::Null,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }

        tracing::info!(
            task_id = %task_id,
            deps = blocked_by.len(),
            team = %team_name,
            "Task added with dependencies"
        );

        // Fire TeamTaskCreated hook
        self.fire_hook(HookEvent::TeamTaskCreated {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: None,
            subject: subject.clone(),
            priority: format!("{:?}", priority),
        });

        Ok(task_id)
    }

    /// Assign a task to an agent using the configured assignment strategy.
    ///
    /// **Note**: For decentralized coordination, prefer `self_claim_task` or
    /// `self_claim_task_for_agent` instead. This method is retained for backward
    /// compatibility with centrally-assigned workflows.
    pub async fn assign_task(
        &self,
        team_name: &str,
        task_id: Uuid,
    ) -> Result<String, AgentError> {
        tracing::warn!(
            task_id = %task_id,
            "assign_task() is deprecated for decentralized coordination; \
             prefer self_claim_task() or self_claim_task_for_agent()"
        );

        // Fetch the task's required capabilities first (if it exists on the board).
        let required_capabilities = self.task_board.get_task(task_id).await
            .map(|t| t.required_capabilities.clone())
            .unwrap_or_default();

        let agent_name = match self.config.assignment_strategy {
            AssignmentStrategy::RoundRobin => {
                self.assign_round_robin(team_name).await?
            }
            AssignmentStrategy::LeastLoaded => {
                self.assign_least_loaded(team_name).await?
            }
            AssignmentStrategy::CapabilityBased => {
                self.assign_capability_based(team_name, &required_capabilities).await?
            }
            AssignmentStrategy::FirstAvailable => {
                self.assign_first_available(team_name).await?
            }
            // SelfClaim is normally invoked via self_claim_task() directly,
            // but when used through assign_task we find the next available task.
            AssignmentStrategy::SelfClaim => {
                self.assign_first_available(team_name).await?
            }
        };

        self.task_board.assign_task(task_id, agent_name.clone()).await?;

        if let Err(e) = self.event_sender.send(CoordinatorEvent::TaskAssigned {
            task_id,
            agent: agent_name.clone(),
        }) {
            tracing::warn!(
                task_id = %task_id,
                agent = %agent_name,
                error = %e,
                "Failed to send TaskAssigned event - no active receivers"
            );
        }

        Ok(agent_name)
    }

    /// Round-robin assignment: cycles through agents using assignment_index.
    async fn assign_round_robin(
        &self,
        team_name: &str,
    ) -> Result<String, AgentError> {
        let mut teams = self.teams.write().await;

        let team = teams.get_mut(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let member_names: Vec<_> = team.members.keys().cloned().collect();

        if member_names.is_empty() {
            return Err(AgentError::Communication("No available agents".to_string()));
        }

        let index = team.assignment_index % member_names.len();
        let agent_name = member_names[index].clone();
        team.assignment_index = team.assignment_index.wrapping_add(1);

        Ok(agent_name)
    }

    /// Least-loaded assignment: pick the agent with the fewest assigned tasks.
    async fn assign_least_loaded(
        &self,
        team_name: &str,
    ) -> Result<String, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let member_names: Vec<_> = team.members.keys().cloned().collect();

        if member_names.is_empty() {
            return Err(AgentError::Communication("No available agents".to_string()));
        }

        drop(teams);

        let mut best_agent: Option<String> = None;
        let mut min_tasks = usize::MAX;

        for name in &member_names {
            let tasks = self.task_board.get_agent_task_count(name).await;
            if tasks < min_tasks {
                min_tasks = tasks;
                best_agent = Some(name.clone());
            }
        }

        best_agent.ok_or_else(|| AgentError::Communication("No available agents".to_string()))
    }

    /// Capability-based assignment: match task requirements against agent capabilities.
    /// Falls back to FirstAvailable when no agent matches all required capabilities.
    async fn assign_capability_based(
        &self,
        team_name: &str,
        required_capabilities: &[String],
    ) -> Result<String, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let member_names: Vec<_> = team.members.keys().cloned().collect();

        if member_names.is_empty() {
            return Err(AgentError::Communication("No available agents".to_string()));
        }

        // If no capabilities are required, fall back to first available.
        if required_capabilities.is_empty() {
            return member_names.first().cloned()
                .ok_or_else(|| AgentError::Communication("No available agents".to_string()));
        }

        // Find agents that possess ALL required capabilities.
        let mut matching_agents: Vec<String> = Vec::new();
        for name in &member_names {
            if let Some(teammate) = team.members.get(name) {
                let has_all = required_capabilities.iter()
                    .all(|cap| teammate.has_capability(cap));
                if has_all {
                    matching_agents.push(name.clone());
                }
            }
        }

        if !matching_agents.is_empty() {
            // Among matching agents, pick the least loaded for fairness.
            drop(teams);
            let mut best_agent: Option<String> = None;
            let mut min_tasks = usize::MAX;

            for name in &matching_agents {
                let tasks = self.task_board.get_agent_task_count(name).await;
                if tasks < min_tasks {
                    min_tasks = tasks;
                    best_agent = Some(name.clone());
                }
            }

            return best_agent.ok_or_else(|| AgentError::Communication("No available agents".to_string()));
        }

        // Fallback: no agent matches all capabilities, use first available.
        tracing::warn!(
            team = %team_name,
            required = ?required_capabilities,
            "No agent matches required capabilities, falling back to FirstAvailable"
        );

        member_names.first().cloned()
            .ok_or_else(|| AgentError::Communication("No available agents".to_string()))
    }

    /// First-available assignment: pick the first member in the team.
    async fn assign_first_available(
        &self,
        team_name: &str,
    ) -> Result<String, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        team.members.keys().next().cloned()
            .ok_or_else(|| AgentError::Communication("No available agents".to_string()))
    }

    // ── Self-Claim Task Assignment (Claude Code approach) ──────────────

    /// An agent explicitly claims a specific task by ID.
    ///
    /// The agent becomes the task owner and the task transitions to InProgress.
    /// This is the Claude Code model: idle agents claim tasks themselves rather
    /// than waiting for central assignment.
    pub async fn self_claim_task(
        &self,
        team_name: &str,
        agent_name: &str,
        task_id: Uuid,
    ) -> Result<AgentTask, AgentError> {
        // Verify the agent is a member of the team
        {
            let teams = self.teams.read().await;
            let team = teams.get(team_name)
                .ok_or_else(|| AgentError::Coordination(
                    CoordinationError::TeamNotFound(team_name.to_string())
                ))?;
            if !team.members.contains_key(agent_name) {
                return Err(AgentError::Communication(
                    format!("Agent '{agent_name}' is not a member of team '{team_name}'")
                ));
            }
        }

        // Verify task is claimable: Pending + no owner + no blockers
        let task = self.task_board.get_task(task_id).await?;
        if task.status != TaskStatus::Pending {
            return Err(AgentError::Task(
                crate::error::TaskError::InvalidTaskState(task_id)
            ));
        }
        if task.owner.is_some() {
            return Err(AgentError::Communication(
                format!("Task {} already claimed by {:?}", task_id, task.owner)
            ));
        }
        if !task.blocked_by.is_empty() {
            return Err(AgentError::Communication(
                format!("Task {} is blocked by {:?}", task_id, task.blocked_by)
            ));
        }

        // Assign and transition to InProgress
        self.task_board.assign_task(task_id, agent_name.to_string()).await?;
        self.task_board.update_task_status(task_id, TaskStatus::InProgress).await?;

        // Update the agent's assigned tasks
        {
            let teams = self.teams.read().await;
            if let Some(team) = teams.get(team_name) {
                if let Some(agent) = team.members.get(agent_name) {
                    let _ = agent.assign_task(task_id).await;
                }
            }
        }

        let updated_task = self.task_board.get_task(task_id).await?;

        if let Err(e) = self.event_sender.send(CoordinatorEvent::TaskAssigned {
            task_id,
            agent: agent_name.to_string(),
        }) {
            tracing::warn!(
                task_id = %task_id,
                agent = %agent_name,
                error = %e,
                "Failed to send TaskAssigned event - no active receivers"
            );
        }

        tracing::info!(
            task_id = %task_id,
            agent = %agent_name,
            team = %team_name,
            "Agent self-claimed task"
        );

        // Persist updated state
        self.persist_team(team_name).await;

        Ok(updated_task)
    }

    /// Find the next claimable task for an agent within a specific team.
    ///
    /// Finds the lowest-ID (earliest created) task in the team that is:
    /// - `Pending` status
    /// - Has no owner
    /// - Has no unresolved `blocked_by` dependencies
    ///
    /// This implements the Claude Code priority rule: prefer tasks in creation
    /// order so that earlier-created tasks are picked first.
    ///
    /// Returns `Option<(Uuid, AgentTask)>` — the task ID and full task — or
    /// `None` if no tasks are available.
    pub async fn find_next_claimable_task(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Option<(Uuid, AgentTask)> {
        // Verify the team exists and the agent is a member
        {
            let teams = self.teams.read().await;
            let Some(team) = teams.get(team_name) else {
                return None;
            };
            if !team.members.contains_key(agent_name) {
                return None;
            }
        }

        let ready_tasks = self.task_board.list_ready_tasks().await;

        // Filter to tasks with no owner, sorted by creation time (earliest first)
        let mut claimable: Vec<_> = ready_tasks
            .into_iter()
            .filter(|t| t.owner.is_none())
            .collect();
        claimable.sort_by_key(|t| t.created_at);

        let task = claimable.into_iter().next()?;

        tracing::debug!(
            task_id = %task.id,
            subject = %task.subject,
            agent = %agent_name,
            team = %team_name,
            "Found claimable task for agent"
        );

        Some((task.id, task))
    }

    /// Convenience: find and claim the next available task for an agent.
    ///
    /// Returns the claimed task, or None if no tasks are available.
    pub async fn claim_next_task(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Option<AgentTask>, AgentError> {
        let Some((task_id, _)) = self.find_next_claimable_task(team_name, agent_name).await else {
            return Ok(None);
        };

        let claimed = self.self_claim_task(team_name, agent_name, task_id).await?;
        Ok(Some(claimed))
    }

    /// Find and atomically claim the next available task for an agent.
    ///
    /// This is the primary decentralized coordination entry point: an idle agent
    /// calls this to find the lowest-ID unblocked, unowned task and claim it in
    /// a single operation.
    ///
    /// Returns the claimed task ID, or `None` if no tasks are available.
    pub async fn self_claim_task_for_agent(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Option<Uuid>, AgentError> {
        let Some((task_id, _)) = self.find_next_claimable_task(team_name, agent_name).await else {
            return Ok(None);
        };

        self.self_claim_task(team_name, agent_name, task_id).await?;
        Ok(Some(task_id))
    }

    // ── Idle Notification ─────────────────────────────────────────────

    /// Called by a teammate when it finishes a task and becomes idle.
    /// Returns the list of claimable tasks (so the agent can immediately
    /// pick up the next one if available).
    pub async fn notify_idle(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Vec<AgentTask>, AgentError> {
        tracing::info!(
            agent = %agent_name,
            team = %team_name,
            "Agent is now idle, checking for available tasks"
        );

        let claimable = self.task_board.list_ready_tasks().await
            .into_iter()
            .filter(|t| t.owner.is_none())
            .collect::<Vec<_>>();

        if !claimable.is_empty() {
            tracing::info!(
                agent = %agent_name,
                available_tasks = claimable.len(),
                "Idle agent has available tasks to claim"
            );
        }

        // Fire TeammateIdle hook
        self.fire_hook(HookEvent::TeammateIdle {
            team_name: team_name.to_string(),
            agent_name: agent_name.to_string(),
            available_tasks: claimable.len(),
        });

        Ok(claimable)
    }

    /// Get the task board
    pub fn task_board(&self) -> &TaskBoard {
        &self.task_board
    }

    /// Complete a task on the task board and fire the TeamTaskCompleted hook.
    ///
    /// This is the preferred way to mark a task as completed because it
    /// triggers the hook system for quality gates.
    pub async fn complete_task(
        &self,
        task_id: Uuid,
        team_name: &str,
        agent_name: &str,
    ) -> Result<(), AgentError> {
        // Fetch task details before completing (for hook subject)
        let subject = self.task_board.get_task(task_id).await
            .map(|t| t.subject.clone())
            .unwrap_or_else(|_| task_id.to_string());

        self.task_board.complete_task(task_id).await?;

        // Fire TeamTaskCompleted hook
        self.fire_hook(HookEvent::TeamTaskCompleted {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: agent_name.to_string(),
            subject,
        });

        Ok(())
    }

    /// Request plan approval from a target agent (typically the team lead).
    ///
    /// Sends a `PlanApprovalRequest` protocol message to the specified agent.
    /// The agent should respond with a `PlanApprovalResponse`.
    pub async fn request_plan_approval(
        &self,
        team_name: &str,
        from_agent: &str,
        to_agent: &str,
        plan: String,
    ) -> Result<Uuid, AgentError> {
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;
        let teammate = team.members.get(to_agent)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::AgentNotFound(to_agent.to_string())
            ))?;

        let request_id = Uuid::new_v4();
        let message = AgentMessage::protocol(
            from_agent.to_string(),
            to_agent.to_string(),
            ProtocolMessage::PlanApprovalRequest {
                request_id,
                plan,
            },
        );

        teammate.send(message).await?;

        tracing::info!(
            team = %team_name,
            from = %from_agent,
            to = %to_agent,
            request_id = %request_id,
            "Plan approval request sent"
        );

        Ok(request_id)
    }

    /// Process a plan approval response.
    ///
    /// If approved, the requesting agent exits plan mode and can proceed with execution.
    /// If rejected, the feedback is sent back to the requesting agent for revision.
    pub async fn handle_plan_response(
        &self,
        team_name: &str,
        request_id: Uuid,
        approved: bool,
        feedback: Option<String>,
        responder: &str,
        requester: &str,
    ) -> Result<(), AgentError> {
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;
        let requester_agent = team.members.get(requester)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::AgentNotFound(requester.to_string())
            ))?;

        if approved {
            // Exit plan mode on the requesting agent
            drop(teams); // Release read lock before modifying
            let teams = self.teams.read().await;
            if let Some(team) = teams.get(team_name) {
                if let Some(agent) = team.members.get(requester) {
                    if let Err(e) = agent.exit_plan_mode().await {
                        tracing::warn!(
                            agent = %requester,
                            error = %e,
                            "Failed to exit plan mode after approval"
                        );
                    }
                }
            }

            tracing::info!(
                team = %team_name,
                request_id = %request_id,
                responder = %responder,
                requester = %requester,
                "Plan approved"
            );
        } else {
            // Send feedback back to the requesting agent
            let feedback_text = feedback.unwrap_or_else(|| "No specific feedback provided".to_string());
            let message = AgentMessage::new_text(
                responder.to_string(),
                requester.to_string(),
                format!("Plan rejected. Feedback: {}", feedback_text),
            );
            requester_agent.send(message).await?;

            tracing::info!(
                team = %team_name,
                request_id = %request_id,
                responder = %responder,
                requester = %requester,
                "Plan rejected with feedback"
            );
        }

        Ok(())
    }

    /// Check for idle agents in a team and return their names.
    /// Useful for surfacing available agents to the team lead after task completion.
    pub async fn idle_agents(&self, team_name: &str) -> Vec<String> {
        let teams = self.teams.read().await;
        let Some(team) = teams.get(team_name) else {
            return Vec::new();
        };
        let mut idle = Vec::new();
        for (name, agent) in &team.members {
            if agent.is_available().await {
                idle.push(name.clone());
            }
        }
        idle
    }

    /// Get a manifest of team members (names, types, capabilities) for agent discovery.
    /// This can be injected into spawned agents' system prompts so they know their teammates.
    pub async fn team_manifest(&self, team_name: &str) -> Result<TeamManifest, AgentError> {
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;
        Ok(self.team_manifest_internal(team).await)
    }

    /// Build manifest from a direct team reference (avoids re-locking).
    async fn team_manifest_internal(&self, team: &AgentTeam) -> TeamManifest {
        let members: Vec<AgentInfo> = team.members.iter().map(|(name, m)| {
            let cfg = m.config();
            AgentInfo {
                name: name.clone(),
                agent_type: cfg.agent_type.clone(),
                capabilities: cfg.capabilities.clone(),
            }
        }).collect();
        TeamManifest {
            name: team.name.clone(),
            description: team.description.clone(),
            members,
        }
    }

    /// Check if delegate mode is enabled.
    /// In delegate mode, the lead agent only coordinates (creates tasks, messages
    /// teammates) and should not directly implement code changes.
    pub fn delegate_mode(&self) -> bool {
        self.config.delegate_mode
    }

    /// Toggle delegate mode on or off.
    pub fn set_delegate_mode(&self, enabled: bool) {
        // Config is behind Arc<RwLock> in some paths, but here we access
        // the config field directly since it's on self.
        // Use interior mutability via a separate flag.
        self.delegate_mode_flag.store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Set the hook manager for firing team-related hooks.
    pub fn set_hook_manager(&mut self, manager: Arc<HookManager>) {
        self.hook_manager = Some(manager);
    }

    /// Fire a hook event asynchronously (non-blocking).
    /// Errors are logged but not propagated — hooks should never block coordinator operations.
    fn fire_hook(&self, event: HookEvent) {
        if let Some(hm) = &self.hook_manager {
            let hm = hm.clone();
            tokio::spawn(async move {
                match hm.run_hooks(&event).await {
                    Ok(results) => {
                        for r in &results {
                            if r.exit_code == 2 {
                                tracing::info!(
                                    event = ?event.event_type(),
                                    exit_code = r.exit_code,
                                    "Hook requested rollback/prevention"
                                );
                            } else if r.exit_code != 0 {
                                tracing::warn!(
                                    event = ?event.event_type(),
                                    exit_code = r.exit_code,
                                    stderr = %r.stderr,
                                    "Hook returned non-zero exit code"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            event = ?event.event_type(),
                            error = %e,
                            "Hook execution failed"
                        );
                    }
                }
            });
        }
    }

    /// Get a human-readable status summary for a team.
    pub async fn team_status(&self, team_name: &str) -> Result<String, AgentError> {
        let teams = self.teams.read().await;
        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;
        Ok(team.summary())
    }

    /// Get the worktree manager
    pub fn worktree_manager(&self) -> Option<&WorktreeManager> {
        self.worktree_manager.as_ref()
    }

    /// Set the file persistence layer for durable team/task state.
    pub fn set_persistence(&mut self, persistence: FilePersistence) {
        self.persistence = Some(persistence);
    }

    /// Get a reference to the persistence layer (if configured).
    pub fn persistence(&self) -> Option<&FilePersistence> {
        self.persistence.as_ref()
    }

    /// Load persisted teams and tasks from disk into memory.
    /// Call this once after coordinator creation, before use.
    pub async fn load_from_disk(&self) -> Result<usize, AgentError> {
        let Some(ref persist) = self.persistence else {
            return Ok(0);
        };

        let team_names = persist.list_teams()?;
        let mut loaded = 0;

        for team_name in &team_names {
            let config_file = match persist.load_team(team_name) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(team = %team_name, error = %e, "Failed to load team, skipping");
                    continue;
                }
            };

            // Reconstruct teammates from config
            let mut members = HashMap::new();
            for (agent_name, teammate_config) in &config_file.members {
                members.insert(agent_name.clone(), Teammate::new(agent_name.clone(), teammate_config.clone()));
            }

            // Load persisted tasks
            let task_files = persist.load_tasks(team_name)?;
            let mut task_list = Vec::new();
            for tf in &task_files {
                if let Ok(task) = tf.to_agent_task() {
                    // Also add to the shared task board
                    let _ = self.task_board.add_task(task.clone()).await;
                    task_list.push(task);
                }
            }

            let team = AgentTeam {
                name: config_file.name.clone(),
                description: config_file.description.clone(),
                members,
                task_list,
                created_at: config_file.created_at.parse()
                    .unwrap_or(chrono::Utc::now()),
                assignment_index: config_file.assignment_index,
            };

            self.teams.write().await.insert(team_name.clone(), team);
            loaded += 1;

            tracing::info!(team = %team_name, "Loaded team from disk");
        }

        Ok(loaded)
    }

    /// Persist a single team's current state to disk.
    async fn persist_team(&self, team_name: &str) {
        let teams = self.teams.read().await;
        self.persist_team_snapshot(team_name, &teams);
    }

    /// Internal helper: persist from an existing lock guard (non-async).
    fn persist_team_snapshot(
        &self,
        team_name: &str,
        teams: &tokio::sync::RwLockReadGuard<'_, HashMap<String, AgentTeam>>,
    ) {
        let Some(ref persist) = self.persistence else {
            return;
        };

        let Some(team) = teams.get(team_name) else {
            return;
        };

        let members: HashMap<String, TeammateConfig> = team.members.iter()
            .map(|(name, m)| (name.clone(), m.config().clone()))
            .collect();

        let config_file = TeamConfigFile {
            name: team.name.clone(),
            description: team.description.clone(),
            members,
            created_at: team.created_at.to_rfc3339(),
            assignment_index: team.assignment_index,
        };

        if let Err(e) = persist.save_team(&config_file) {
            tracing::warn!(team = %team_name, error = %e, "Failed to persist team config");
        }
    }

    /// Handle incoming message internally
    async fn handle_message_internal(
        message: AgentMessage,
        teams: &Arc<RwLock<HashMap<String, AgentTeam>>>,
        _task_board: &TaskBoard,
        event_sender: &broadcast::Sender<CoordinatorEvent>,
    ) -> Result<(), AgentError> {
        if let Err(e) = event_sender.send(CoordinatorEvent::MessageSent(message.clone())) {
            tracing::warn!(
                from = %message.from,
                to = %message.to,
                error = %e,
                "Failed to send MessageSent event - no active receivers"
            );
        }

        if message.to == "*" {
            // Broadcast to all agents in all teams
            let teams_lock = teams.read().await;
            for (_team_name, team) in teams_lock.iter() {
                for (_agent_name, agent) in team.members.iter() {
                    let _ = agent.handle_message(message.clone()).await;
                }
            }
        } else {
            // Send to specific agent
            let teams_lock = teams.read().await;
            for (_team_name, team) in teams_lock.iter() {
                if let Some(agent) = team.members.get(&message.to) {
                    let response = agent.handle_message(message.clone()).await?;
                    // Handle response if needed
                    if let Err(e) = event_sender.send(CoordinatorEvent::MessageSent(response)) {
                        tracing::warn!(
                            from = %message.from,
                            to = %message.to,
                            error = %e,
                            "Failed to send MessageSent event for response - no active receivers"
                        );
                    }
                    return Ok(());
                }
            }
            return Err(AgentError::Coordination(
                CoordinationError::AgentNotFound(message.to)
            ));
        }

        Ok(())
    }

    /// Send heartbeat to all agents
    async fn send_heartbeats(
        teams: &Arc<RwLock<HashMap<String, AgentTeam>>>,
        event_sender: &broadcast::Sender<CoordinatorEvent>,
    ) {
        let teams_lock = teams.read().await;
        for (_team_name, team) in teams_lock.iter() {
            for (agent_name, agent) in team.members.iter() {
                let status = agent.status().await;
                if let Err(e) = event_sender.send(CoordinatorEvent::StatusChanged {
                    agent: agent_name.clone(),
                    status,
                }) {
                    tracing::warn!(
                        agent = %agent_name,
                        error = %e,
                        "Failed to send StatusChanged event - no active receivers"
                    );
                }
            }
        }
    }

    /// Auto-claim tasks for idle agents (SelfClaim strategy).
    ///
    /// Scans all teams for idle agents and attempts to assign them
    /// ready, unowned tasks from the task board.
    async fn auto_claim_idle_agents(
        teams: &Arc<RwLock<HashMap<String, AgentTeam>>>,
        task_board: &Arc<TaskBoard>,
        event_sender: &broadcast::Sender<CoordinatorEvent>,
    ) {
        let teams_lock = teams.read().await;
        for (team_name, team) in teams_lock.iter() {
            for (agent_name, agent) in team.members.iter() {
                if !agent.is_available().await {
                    continue;
                }

                // Find the first ready, unowned task this agent could claim
                let ready_tasks = task_board.list_ready_tasks().await;
                let claimable = ready_tasks.into_iter()
                    .find(|t| t.owner.is_none());

                if let Some(task) = claimable {
                    match task_board.assign_task(task.id, agent_name.clone()).await {
                        Ok(()) => {
                            tracing::info!(
                                team = %team_name,
                                agent = %agent_name,
                                task_id = %task.id,
                                "Auto-claimed task for idle agent via heartbeat"
                            );
                            let _ = event_sender.send(CoordinatorEvent::TaskAutoClaimed {
                                task_id: task.id,
                                team: team_name.clone(),
                                agent: agent_name.clone(),
                            });
                        }
                        Err(e) => {
                            tracing::debug!(
                                agent = %agent_name,
                                task_id = %task.id,
                                error = %e,
                                "Auto-claim failed (likely raced with another agent)"
                            );
                        }
                    }
                    // Only claim one task per agent per heartbeat tick
                    break;
                }
            }
        }
    }

    /// Start the background inbox delivery loop.
    ///
    /// Periodically checks all teammates' inboxes and auto-delivers pending
    /// messages by injecting them into the teammate's message handling flow.
    fn start_inbox_delivery_loop(&mut self, cancel_rx: &mut watch::Receiver<bool>) {
        let teams = self.teams.clone();
        let mut cancel_rx = cancel_rx.clone();

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tracing::info!("Inbox delivery loop cancelled");
                            return;
                        }
                    }
                }

                let teams = teams.read().await;
                for (team_name, team) in teams.iter() {
                    for (agent_name, teammate) in &team.members {
                        // Check if the teammate has pending messages in its inbox
                        if let Ok(Some(msg)) = teammate.try_recv() {
                            tracing::debug!(
                                team = %team_name,
                                agent = %agent_name,
                                from = %msg.from,
                                "Auto-delivering inbox message to teammate"
                            );
                            if let Err(e) = teammate.handle_message(msg).await {
                                tracing::warn!(
                                    team = %team_name,
                                    agent = %agent_name,
                                    error = %e,
                                    "Failed to auto-deliver inbox message"
                                );
                            }
                        }
                    }
                }
                drop(teams);
            }
        });

        self._delivery_handle = Some(handle);
    }

    /// Disband a specific team: shutdown all agents, remove from memory, delete persisted files.
    pub async fn disband_team(&self, team_name: &str) -> Result<(), AgentError> {
        tracing::info!(team = %team_name, "Disbanding team");

        let mut teams = self.teams.write().await;
        let team = teams.get_mut(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        // Send shutdown to all teammates
        for (agent_name, teammate) in team.members.iter() {
            tracing::debug!(team = %team_name, agent = %agent_name, "Sending shutdown to agent");
            let shutdown_msg = AgentMessage::protocol(
                "coordinator".to_string(),
                agent_name.clone(),
                ProtocolMessage::ShutdownRequest {
                    reason: "Team disbanded".to_string(),
                },
            );
            let _ = teammate.handle_message(shutdown_msg).await;
            if let Err(e) = self.event_sender.send(CoordinatorEvent::AgentLeft {
                team: team_name.to_string(),
                agent: agent_name.clone(),
            }) {
                tracing::warn!(team = %team_name, agent = %agent_name, error = %e,
                    "Failed to send AgentLeft event during disband");
            }
        }

        // Remove team from memory
        let member_names: Vec<String> = team.members.keys().cloned().collect();
        teams.remove(team_name);
        drop(teams);

        // Clean up task board — remove tasks owned by disbanded team members
        for member in &member_names {
            let agent_tasks = self.task_board.get_agent_tasks(member).await;
            for task in agent_tasks {
                let _ = self.task_board.fail_task(task.id, "Team disbanded".to_string()).await;
            }
        }

        // Clean up process agents for this team
        if let Some(ref pm) = self.process_manager {
            for member in &member_names {
                let _ = pm.graceful_shutdown_agent(member, Duration::from_secs(5)).await;
            }
        }

        // Delete persisted files
        if let Some(ref persist) = self.persistence {
            if let Err(e) = persist.delete_team(team_name) {
                tracing::warn!(team = %team_name, error = %e, "Failed to delete persisted team files");
            }
        }

        if let Err(e) = self.event_sender.send(CoordinatorEvent::TeamDeleted {
            team: team_name.to_string(),
        }) {
            tracing::warn!(team = %team_name, error = %e, "Failed to send TeamDeleted event");
        }

        tracing::info!(team = %team_name, "Team disbanded successfully");
        Ok(())
    }

    /// Poll for pending RPC requests from process agents and handle them.
    /// Call this periodically from the main coordinator loop.
    pub async fn poll_rpc_requests(&mut self) {
        // Drain all pending requests first to avoid borrow conflicts
        let pending: Vec<_> = {
            let rpc_rx = match &mut self.rpc_request_rx {
                Some(rx) => rx,
                None => return,
            };
            let mut batch = Vec::new();
            while let Ok(req) = rpc_rx.try_recv() {
                batch.push(req);
            }
            batch
        };

        for (agent_name, request_id, method, params) in pending {
            let result = self.handle_rpc_request(&agent_name, &method, &params).await;
            if let Some(ref pm) = self.process_manager {
                if let Err(e) = pm.send_rpc_response(&agent_name, request_id, result).await {
                    tracing::warn!(
                        agent = %agent_name,
                        request_id,
                        error = %e,
                        "Failed to send RPC response to agent"
                    );
                }
            }
        }
    }

    /// Check whether the given agent is a team lead in any team.
    async fn is_team_lead(&self, agent_name: &str) -> bool {
        let teams = self.teams.read().await;
        for team in teams.values() {
            if let Some(teammate) = team.members.get(agent_name) {
                if teammate.config().is_lead {
                    return true;
                }
            }
        }
        false
    }

    /// Handle an RPC request from a process agent.
    async fn handle_rpc_request(
        &self,
        caller: &str,
        method: &str,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        match method {
            "create_task" => {
                let subject = params["subject"].as_str().unwrap_or_default();
                let description = params["description"].as_str().unwrap_or_default();
                let priority = params["priority"].as_str().unwrap_or("medium");
                let priority = match priority {
                    "low" => TaskPriority::Low,
                    "high" => TaskPriority::High,
                    "critical" => TaskPriority::Critical,
                    _ => TaskPriority::Medium,
                };

                // Use the first available team
                let teams = self.teams.read().await;
                if let Some(team_name) = teams.keys().next() {
                    let team_name = team_name.clone();
                    drop(teams);

                    match self.add_task(&team_name, subject.to_string(), description.to_string(), priority).await {
                        Ok(task_id) => serde_json::json!({
                            "task_id": task_id.to_string(),
                            "status": "created"
                        }),
                        Err(e) => serde_json::json!({
                            "error": e.to_string()
                        }),
                    }
                } else {
                    serde_json::json!({"error": "no team found"})
                }
            }
            "update_task" => {
                let task_id_str = params["task_id"].as_str().unwrap_or_default();
                let status = params["status"].as_str().unwrap_or_default();

                match Uuid::parse_str(task_id_str) {
                    Ok(task_id) => {
                        if !status.is_empty() {
                            let new_status = match status {
                                "pending" => TaskStatus::Pending,
                                "in_progress" => TaskStatus::InProgress,
                                "completed" => TaskStatus::Completed,
                                "failed" => TaskStatus::Failed(String::new()),
                                "blocked" => TaskStatus::Blocked,
                                "cancelled" => TaskStatus::Cancelled,
                                _ => TaskStatus::Pending,
                            };
                            match self.task_board.update_task_status(task_id, new_status).await {
                                Ok(()) => serde_json::json!({"status": "updated"}),
                                Err(e) => serde_json::json!({"error": e.to_string()}),
                            }
                        } else {
                            serde_json::json!({"status": "no_changes"})
                        }
                    }
                    Err(e) => serde_json::json!({"error": format!("invalid task_id: {e}")}),
                }
            }
            "get_task" => {
                let task_id_str = params["task_id"].as_str().unwrap_or_default();
                match Uuid::parse_str(task_id_str) {
                    Ok(task_id) => {
                        match self.task_board.get_task(task_id).await {
                            Ok(task) => serde_json::json!({
                                "id": task.id.to_string(),
                                "subject": task.subject,
                                "description": task.description,
                                "status": format!("{:?}", task.status),
                                "priority": format!("{:?}", task.priority),
                                "owner": task.owner,
                            }),
                            Err(e) => serde_json::json!({"error": e.to_string()}),
                        }
                    }
                    Err(e) => serde_json::json!({"error": format!("invalid task_id: {e}")}),
                }
            }
            "team_manifest" => {
                let teams = self.teams.read().await;
                if let Some(team_name) = teams.keys().next() {
                    match self.team_manifest(team_name).await {
                        Ok(manifest) => serde_json::json!({
                            "name": manifest.name,
                            "description": manifest.description,
                            "members": manifest.members,
                        }),
                        Err(e) => serde_json::json!({"error": e.to_string()}),
                    }
                } else {
                    serde_json::json!({"error": "no team found"})
                }
            }
            "list_tasks" => {
                let status_filter = params["status"].as_str().unwrap_or("");
                let tasks = if status_filter.is_empty() {
                    self.task_board.list_all_tasks().await
                } else {
                    let status = match status_filter {
                        "pending" => TaskStatus::Pending,
                        "in_progress" => TaskStatus::InProgress,
                        "completed" => TaskStatus::Completed,
                        "failed" => TaskStatus::Failed(String::new()),
                        "blocked" => TaskStatus::Blocked,
                        _ => TaskStatus::Pending,
                    };
                    self.task_board.list_tasks_by_status(status).await
                };
                let task_list: Vec<serde_json::Value> = tasks.iter().map(|t| {
                    serde_json::json!({
                        "id": t.id.to_string(),
                        "subject": t.subject,
                        "status": format!("{:?}", t.status),
                        "priority": format!("{:?}", t.priority),
                        "owner": t.owner,
                    })
                }).collect();
                serde_json::json!({"tasks": task_list})
            }
            "claim_task" => {
                let agent_name = params["agent_name"].as_str().unwrap_or_default();
                let task_id_str = params["task_id"].as_str().unwrap_or("");

                let teams = self.teams.read().await;
                if let Some(team_name) = teams.keys().next() {
                    let team_name = team_name.clone();
                    drop(teams);

                    if task_id_str.is_empty() {
                        match self.self_claim_task_for_agent(&team_name, agent_name).await {
                            Ok(Some(task_id)) => serde_json::json!({
                                "task_id": task_id.to_string(),
                                "status": "claimed"
                            }),
                            Ok(None) => serde_json::json!({"status": "no_tasks_available"}),
                            Err(e) => serde_json::json!({"error": e.to_string()}),
                        }
                    } else {
                        match Uuid::parse_str(task_id_str) {
                            Ok(task_id) => {
                                match self.self_claim_task(&team_name, agent_name, task_id).await {
                                    Ok(task) => serde_json::json!({
                                        "task_id": task.id.to_string(),
                                        "subject": task.subject,
                                        "status": "claimed"
                                    }),
                                    Err(e) => serde_json::json!({"error": e.to_string()}),
                                }
                            }
                            Err(e) => serde_json::json!({"error": format!("invalid task_id: {e}")}),
                        }
                    }
                } else {
                    serde_json::json!({"error": "no team found"})
                }
            }
            "disband_team" => {
                // Lead-only operation
                if !self.is_team_lead(caller).await {
                    serde_json::json!({"error": "permission denied: only team leads can disband teams"})
                } else {
                    let team_name = params["team_name"].as_str().unwrap_or_default();
                    if team_name.is_empty() {
                        serde_json::json!({"error": "missing team_name"})
                    } else {
                        match self.disband_team(team_name).await {
                            Ok(()) => serde_json::json!({"status": "disbanded"}),
                            Err(e) => serde_json::json!({"error": e.to_string()}),
                        }
                    }
                }
            }
            "add_agent" => {
                // Lead-only operation
                if !self.is_team_lead(caller).await {
                    serde_json::json!({"error": "permission denied: only team leads can add agents"})
                } else {
                    let team_name = params["team_name"].as_str().unwrap_or_default();
                    let new_agent_name = params["agent_name"].as_str().unwrap_or_default();
                    let agent_type = params["agent_type"].as_str().unwrap_or("general-purpose");

                    if team_name.is_empty() || new_agent_name.is_empty() {
                        serde_json::json!({"error": "missing team_name or agent_name"})
                    } else {
                        let config = TeammateConfig {
                            agent_type: agent_type.to_string(),
                            ..Default::default()
                        };
                        match self.add_teammate(team_name, new_agent_name.to_string(), config).await {
                            Ok(()) => serde_json::json!({"status": "added"}),
                            Err(e) => serde_json::json!({"error": e.to_string()}),
                        }
                    }
                }
            }
            _ => serde_json::json!({"error": format!("unknown method: {method}")}),
        }
    }

    /// Gracefully shutdown the coordinator and all teams
    pub async fn shutdown(&self) -> Result<(), AgentError> {
        tracing::info!("Shutting down agent coordinator");

        let teams = self.teams.write().await;

        // Send shutdown requests to all teammates
        for (team_name, team) in teams.iter() {
            tracing::debug!("Shutting down team '{}'", team_name);

            for (agent_name, teammate) in team.members.iter() {
                tracing::debug!("Shutting down agent '{}' in team '{}'", agent_name, team_name);

                // Send shutdown protocol message
                let shutdown_msg = AgentMessage::protocol(
                    "coordinator".to_string(),
                    agent_name.clone(),
                    ProtocolMessage::ShutdownRequest {
                        reason: "Coordinator shutting down".to_string(),
                    },
                );

                let _ = teammate.handle_message(shutdown_msg).await;
                if let Err(e) = self.event_sender.send(CoordinatorEvent::AgentLeft {
                    team: team_name.clone(),
                    agent: agent_name.clone(),
                }) {
                    tracing::warn!(
                        team = %team_name,
                        agent = %agent_name,
                        error = %e,
                        "Failed to send AgentLeft event during shutdown - no active receivers"
                    );
                }
            }
        }

        // Cleanup worktrees if enabled
        if let Some(manager) = &self.worktree_manager {
            let _ = manager.cleanup_all().await;
        }

        Ok(())
    }

    /// Spawn a background task for an agent.
    ///
    /// The agent processes the message asynchronously in a separate tokio task.
    /// When complete, a notification message is sent from the agent back to the
    /// specified `reply_to` name (typically the team lead).
    ///
    /// Returns a `tokio::task::JoinHandle` that can be used to await or cancel the task.
    pub fn spawn_background_task(
        &self,
        team_name: &str,
        agent_name: &str,
        reply_to: &str,
        content: String,
    ) -> Result<tokio::task::JoinHandle<()>, AgentError> {
        self.spawn_background_task_with_timeout(team_name, agent_name, reply_to, content, None)
    }

    /// Spawn a background task with an optional timeout (in seconds).
    ///
    /// Returns a `tokio::task::JoinHandle` that can be used to await the task.
    /// The task is tracked internally and can be cancelled via `cancel_background_task`.
    pub fn spawn_background_task_with_timeout(
        &self,
        team_name: &str,
        agent_name: &str,
        reply_to: &str,
        content: String,
        timeout_secs: Option<u64>,
    ) -> Result<tokio::task::JoinHandle<()>, AgentError> {
        let teams = self.teams.try_read()
            .map_err(|_| AgentError::Communication("Teams lock poisoned".to_string()))?;

        let team = teams.get(team_name)
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::TeamNotFound(team_name.to_string())
            ))?;

        let teammate = team.members.get(agent_name)
            .cloned()
            .ok_or_else(|| AgentError::Coordination(
                CoordinationError::AgentNotFound(agent_name.to_string())
            ))?;

        let from = reply_to.to_string();
        let agent_name_owned = agent_name.to_string();
        let task_key = format!("{}:{}", team_name, agent_name);
        let message = AgentMessage::new_text(from.clone(), agent_name_owned.clone(), content);

        // Drop the teams lock before spawning the task
        drop(teams);

        let background_tasks = self.background_tasks.clone();
        let event_sender = self.event_sender.clone();
        let team_name_owned = team_name.to_string();
        let task_key_for_cleanup = task_key.clone();

        let handle = tokio::spawn(async move {
            // Emit started event
            let _ = event_sender.send(CoordinatorEvent::AgentOutput {
                team: team_name_owned.clone(),
                agent: agent_name_owned.clone(),
                chunk: "[started]".to_string(),
            });

            let result = if let Some(secs) = timeout_secs {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(secs),
                    teammate.handle_chat_message(message),
                ).await {
                    Ok(inner) => inner,
                    Err(_) => {
                        tracing::warn!(
                            agent = %agent_name_owned,
                            timeout_secs = secs,
                            "Background task timed out"
                        );
                        // Clean up tracking entry
                        background_tasks.write().await.remove(&task_key_for_cleanup);
                        return;
                    }
                }
            } else {
                teammate.handle_chat_message(message).await
            };

            match result {
                Ok(response) => {
                    tracing::info!(
                        agent = %agent_name_owned,
                        to = %response.to,
                        "Background task completed"
                    );
                    let output = match &response.content {
                        MessageContent::Text(t) => t.clone(),
                        other => format!("{other:?}"),
                    };
                    let _ = event_sender.send(CoordinatorEvent::AgentCompleted {
                        team: team_name_owned.clone(),
                        agent: agent_name_owned.clone(),
                        success: true,
                        output,
                    });
                }
                Err(e) => {
                    tracing::warn!(
                        agent = %agent_name_owned,
                        error = %e,
                        "Background task failed"
                    );
                    let _ = event_sender.send(CoordinatorEvent::AgentCompleted {
                        team: team_name_owned.clone(),
                        agent: agent_name_owned.clone(),
                        success: false,
                        output: e.to_string(),
                    });
                }
            }

            // Clean up tracking entry
            background_tasks.write().await.remove(&task_key_for_cleanup);
        });

        // Track the abort handle for cancellation
        let abort_handle = handle.abort_handle();
        // Use blocking lock since we're in a sync context (try_read above already succeeded)
        if let Ok(mut tasks) = self.background_tasks.try_write() {
            tasks.insert(task_key.clone(), abort_handle);
        }

        tracing::info!(
            team = %team_name,
            agent = %agent_name,
            timeout = ?timeout_secs,
            "Spawned background task"
        );

        Ok(handle)
    }

    /// Cancel a running background task for a specific agent.
    ///
    /// Returns true if the task was found and aborted, false if it wasn't running.
    pub async fn cancel_background_task(&self, team_name: &str, agent_name: &str) -> bool {
        let key = format!("{}:{}", team_name, agent_name);
        let mut tasks = self.background_tasks.write().await;
        if let Some(handle) = tasks.remove(&key) {
            handle.abort();
            tracing::info!(team = %team_name, agent = %agent_name, "Cancelled background task");
            true
        } else {
            tracing::debug!(team = %team_name, agent = %agent_name, "No background task to cancel");
            false
        }
    }

    /// List all running background tasks.
    pub async fn running_background_tasks(&self) -> Vec<String> {
        self.background_tasks.read().await.keys().cloned().collect()
    }
}
