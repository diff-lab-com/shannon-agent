//! Agent coordinator for managing multi-agent teams

use crate::{
    TaskBoard,
    custom_agent::{CustomAgentDef, CustomAgentError, CustomAgentLoader},
    error::{AgentError, CoordinationError},
    message::{AgentMessage, MessageContent, MessageType, ProtocolMessage},
    message_history::ContentKind,
    persistence::{FilePersistence, InboxMessage, TeamConfigFile},
    process_manager::{AgentEvent, AgentProcessConfig, AgentProcessManager},
    task::{AgentTask, TaskPriority, TaskStatus},
    teammate::{Teammate, TeammateConfig, TeammateStatus},
    worktree::{WorktreeConfig, WorktreeManager},
};
use serde::{Deserialize, Serialize};
use shannon_core::hooks::{HookEvent, HookManager};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, broadcast, mpsc, watch};
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
    TaskAssigned {
        task_id: Uuid,
        agent: String,
        subject: String,
    },
    /// Task completed
    TaskCompleted { task_id: Uuid, agent: String },
    /// Task failed
    TaskFailed {
        task_id: Uuid,
        agent: String,
        reason: String,
    },
    /// Agent status changed
    StatusChanged {
        agent: String,
        status: TeammateStatus,
    },
    /// Team was disbanded/deleted
    TeamDeleted { team: String },
    /// Background agent task produced output (streaming chunk)
    AgentOutput {
        team: String,
        agent: String,
        chunk: String,
    },
    /// Background agent task completed
    AgentCompleted {
        team: String,
        agent: String,
        success: bool,
        output: String,
    },
    /// An idle agent was auto-assigned a task
    TaskAutoClaimed {
        task_id: Uuid,
        team: String,
        agent: String,
    },
    /// A peer-to-peer direct message was sent between agents
    PeerDirectMessage {
        team: String,
        from: String,
        to: String,
        summary: Option<String>,
    },
}

/// Main coordinator for managing multi-agent teams
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
    /// Optional append-only message history log (per-team JSONL).
    message_history: Option<crate::message_history::MessageHistoryStore>,
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
    /// Custom agent definitions loaded from `.claude/agents/*.md`.
    custom_agents: std::collections::HashMap<String, CustomAgentDef>,
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
            Some(WorktreeManager::new(config.worktree_config.clone().unwrap_or_default()).await?)
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
                )
                .await
                {
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
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(heartbeat_interval));
            // Consume the first immediate tick so heartbeat waits a full interval
            // before firing. This avoids racing with test setup and initial task creation.
            interval.tick().await;
            loop {
                interval.tick().await;
                Self::send_heartbeats(&teams_heartbeat, &event_heartbeat).await;
                if matches!(assignment_strategy, AssignmentStrategy::SelfClaim) {
                    Self::auto_claim_idle_agents(
                        &teams_heartbeat,
                        &task_board_heartbeat,
                        &event_heartbeat,
                    )
                    .await;
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
            message_history: None,
            background_tasks: Arc::new(RwLock::new(HashMap::new())),
            delegate_mode_flag: std::sync::atomic::AtomicBool::new(delegate_mode),
            _delivery_handle: None,
            delivery_cancel,
            hook_manager: None,
            process_manager: None,
            rpc_request_rx: None,
            custom_agents: HashMap::new(),
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
                        AgentEvent::Ready {
                            agent_name,
                            capabilities: _,
                        } => {
                            tracing::info!(agent = %agent_name, "Process agent ready");
                            None
                        }
                        AgentEvent::Progress {
                            agent_name,
                            task_id,
                            chunk,
                        } => {
                            Some(CoordinatorEvent::AgentOutput {
                                team: String::new(), // filled by subscriber
                                agent: agent_name,
                                chunk: format!("[{task_id}] {chunk}"),
                            })
                        }
                        AgentEvent::TaskComplete {
                            agent_name,
                            task_id,
                            success,
                            output,
                        } => match Uuid::parse_str(&task_id) {
                            Ok(parsed_id) => {
                                if success {
                                    Some(CoordinatorEvent::TaskCompleted {
                                        task_id: parsed_id,
                                        agent: agent_name,
                                    })
                                } else {
                                    Some(CoordinatorEvent::TaskFailed {
                                        task_id: parsed_id,
                                        agent: agent_name,
                                        reason: output,
                                    })
                                }
                            }
                            Err(e) => {
                                tracing::warn!(agent = %agent_name, task_id = %task_id, error = %e, "Invalid task ID in TaskComplete event");
                                None
                            }
                        },
                        AgentEvent::Idle {
                            agent_name,
                            available_tasks_count: _,
                        } => Some(CoordinatorEvent::StatusChanged {
                            agent: agent_name,
                            status: TeammateStatus::Idle,
                        }),
                        AgentEvent::ProcessExited {
                            agent_name,
                            exit_code,
                        } => {
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
                        AgentEvent::HealthCheckFailed {
                            agent_name,
                            consecutive_failures,
                        } => {
                            tracing::warn!(
                                agent = %agent_name,
                                failures = consecutive_failures,
                                "Process agent health check failed"
                            );
                            None
                        }
                        AgentEvent::AgentRestarted {
                            agent_name,
                            restart_count,
                        } => {
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
                        AgentEvent::RpcRequest {
                            agent_name,
                            request_id,
                            method,
                            params,
                        } => {
                            tracing::debug!(
                                agent = %agent_name,
                                method = %method,
                                request_id,
                                "Forwarding RPC request from agent"
                            );
                            if let Err(e) =
                                rpc_tx.send((agent_name, request_id, method, params)).await
                            {
                                tracing::warn!("Failed to forward RPC request: {e}");
                            }
                            None
                        }
                    };
                    if let Some(event) = coord_event {
                        if let Err(e) = event_sender_for_pm.send(event) {
                            tracing::debug!("Failed to send coordinator event: {e}");
                        }
                    }
                }
            });

            coordinator.process_manager = Some(pm);
        }

        Ok(coordinator)
    }

    /// Create a new agent team
    pub async fn create_team(&self, name: String, description: String) -> Result<(), AgentError> {
        let mut teams = self.teams.write().await;

        if teams.contains_key(&name) {
            return Err(AgentError::Coordination(
                CoordinationError::InvalidConfiguration(format!("team '{name}' already exists")),
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

        let team = teams.get_mut(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        if team.members.len() >= self.config.max_team_size {
            return Err(AgentError::Coordination(
                CoordinationError::MaxTeamSizeExceeded(self.config.max_team_size),
            ));
        }

        if team.members.contains_key(&agent_name) {
            return Err(AgentError::Coordination(
                CoordinationError::AgentAlreadyMember(agent_name, team_name.to_string()),
            ));
        }

        // Inject team manifest into agent's system prompt so it knows its teammates
        let mut config = config;
        let manifest = self.team_manifest_internal(team).await;
        let manifest_suffix = format!(
            "\n\n## Your Team: {}\n{}",
            manifest.name,
            manifest
                .members
                .iter()
                .filter(|m| m.name != agent_name)
                .map(|m| format!(
                    "- {} ({}): {}",
                    m.name,
                    m.agent_type,
                    m.capabilities.join(", ")
                ))
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
        let config_isolation = config.isolation.clone();

        let teammate = Teammate::new(agent_name.clone(), config);
        teammate.set_team_name(team_name.to_string());
        team.members.insert(agent_name.clone(), teammate);

        // Create isolated worktree for this agent if worktree isolation is enabled
        // (either globally via config or per-agent via the `isolation` field)
        let needs_worktree = self.config.enable_worktree_isolation
            || config_isolation.as_deref() == Some("worktree");
        if needs_worktree {
            // Lazily initialize worktree manager if needed for per-agent isolation
            if let Some(ref wm) = self.worktree_manager {
                match wm.create_agent_session(&agent_name, None).await {
                    Ok(session) => {
                        tracing::info!(
                            team = %team_name,
                            agent = %agent_name,
                            worktree = %session.path.display(),
                            "Created isolated worktree for agent"
                        );
                        // Fire WorktreeCreate hook
                        self.fire_hook(HookEvent::WorktreeCreate {
                            path: session.path.to_string_lossy().to_string(),
                            branch: session.branch_name.clone(),
                        });
                        // Store worktree path in teammate metadata
                        if let Some(agent) = team.members.get(&agent_name) {
                            agent
                                .set_metadata(
                                    "worktree_path".to_string(),
                                    serde_json::json!(session.path.to_string_lossy().to_string()),
                                )
                                .await;
                            agent
                                .set_metadata(
                                    "worktree_branch".to_string(),
                                    serde_json::json!(session.branch_name),
                                )
                                .await;
                        } else {
                            tracing::warn!(
                                team = %team_name,
                                agent = %agent_name,
                                "agent not found in team after worktree creation, skipping metadata"
                            );
                        }
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
                let binary_path =
                    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("shannon"));

                // Get worktree path if isolation is enabled
                let worktree_path = {
                    let teams = self.teams.read().await;
                    match teams
                        .get(team_name)
                        .and_then(|t| t.members.get(&agent_name))
                    {
                        Some(agent) => agent
                            .get_metadata("worktree_path")
                            .await
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
                    startup_timeout_secs: 60,
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
        self.message_sender
            .send(message)
            .await
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        let agent = team.members.get(to).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::AgentNotFound(to.to_string()))
        })?;

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

        // Append to long-lived message history (best-effort).
        self.record_to_history(team_name, &message);

        if let Err(e) = self
            .event_sender
            .send(CoordinatorEvent::MessageSent(message.clone()))
        {
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

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

                    // Append to long-lived message history (best-effort).
                    let per_recipient = crate::message::AgentMessage {
                        id: message.id,
                        from: message.from.clone(),
                        to: agent_name.to_string(),
                        message_type: message.message_type.clone(),
                        priority: message.priority,
                        content: MessageContent::Text(content.clone()),
                        timestamp: message.timestamp,
                    };
                    self.record_to_history(team_name, &per_recipient);

                    if let Err(e) = self
                        .event_sender
                        .send(CoordinatorEvent::MessageSent(message))
                    {
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
                if let Ok(messages) = persist.peek_inbox(team_name, agent_name) {
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

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        Ok(team.members.keys().cloned().collect())
    }

    /// Get agent by name
    pub async fn get_agent(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Teammate, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        team.members.get(agent_name).cloned().ok_or_else(|| {
            AgentError::Coordination(CoordinationError::AgentNotFound(agent_name.to_string()))
        })
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

        self.task_board.add_task(task.clone()).await?;

        let mut teams = self.teams.write().await;
        if let Some(team) = teams.get_mut(team_name) {
            team.task_list.push(task.clone());
        }
        drop(teams);

        // Persist task file to disk
        self.persist_task(team_name, &task);

        // Fire TeamTaskCreated hook
        self.fire_hook(HookEvent::TeamTaskCreated {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: None,
            subject: subject.clone(),
            priority: format!("{priority:?}"),
        });

        // Fire TaskCreated hook (Claude Code standard)
        self.fire_hook(HookEvent::TaskCreated {
            task_id: task_id.to_string(),
            subject,
            priority: format!("{priority:?}"),
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

        let task_clone = task.clone();
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
            team.task_list.push(task_clone);
        }

        tracing::info!(
            task_id = %task_id,
            deps = blocked_by.len(),
            team = %team_name,
            "Task added with dependencies"
        );

        // Persist task file to disk
        if let Ok(tb_task) = self.task_board.get_task(task_id).await {
            self.persist_task(team_name, &tb_task);
        }

        // Fire TeamTaskCreated hook
        self.fire_hook(HookEvent::TeamTaskCreated {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: None,
            subject: subject.clone(),
            priority: format!("{priority:?}"),
        });

        // Fire TaskCreated hook (Claude Code standard)
        self.fire_hook(HookEvent::TaskCreated {
            task_id: task_id.to_string(),
            subject,
            priority: format!("{priority:?}"),
        });

        Ok(task_id)
    }

    /// Assign a task to an agent using the configured assignment strategy.
    ///
    /// **Note**: For decentralized coordination, prefer `self_claim_task` or
    /// `self_claim_task_for_agent` instead. This method is retained for backward
    /// compatibility with centrally-assigned workflows.
    pub async fn assign_task(&self, team_name: &str, task_id: Uuid) -> Result<String, AgentError> {
        tracing::warn!(
            task_id = %task_id,
            "assign_task() is deprecated for decentralized coordination; \
             prefer self_claim_task() or self_claim_task_for_agent()"
        );

        // Fetch the task's required capabilities first (if it exists on the board).
        let required_capabilities = self
            .task_board
            .get_task(task_id)
            .await
            .map(|t| t.required_capabilities.clone())
            .unwrap_or_default();

        let agent_name = match self.config.assignment_strategy {
            AssignmentStrategy::RoundRobin => self.assign_round_robin(team_name).await?,
            AssignmentStrategy::LeastLoaded => self.assign_least_loaded(team_name).await?,
            AssignmentStrategy::CapabilityBased => {
                self.assign_capability_based(team_name, &required_capabilities)
                    .await?
            }
            AssignmentStrategy::FirstAvailable => self.assign_first_available(team_name).await?,
            // SelfClaim is normally invoked via self_claim_task() directly,
            // but when used through assign_task we find the next available task.
            AssignmentStrategy::SelfClaim => self.assign_first_available(team_name).await?,
        };

        self.task_board
            .assign_task(task_id, agent_name.clone())
            .await?;
        let assigned_task = self.task_board.get_task(task_id).await?;

        if let Err(e) = self.event_sender.send(CoordinatorEvent::TaskAssigned {
            task_id,
            agent: agent_name.clone(),
            subject: assigned_task.subject.clone(),
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
    async fn assign_round_robin(&self, team_name: &str) -> Result<String, AgentError> {
        let mut teams = self.teams.write().await;

        let team = teams.get_mut(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

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
    async fn assign_least_loaded(&self, team_name: &str) -> Result<String, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

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

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        let member_names: Vec<_> = team.members.keys().cloned().collect();

        if member_names.is_empty() {
            return Err(AgentError::Communication("No available agents".to_string()));
        }

        // If no capabilities are required, fall back to first available.
        if required_capabilities.is_empty() {
            return member_names
                .first()
                .cloned()
                .ok_or_else(|| AgentError::Communication("No available agents".to_string()));
        }

        // Find agents that possess ALL required capabilities.
        let mut matching_agents: Vec<String> = Vec::new();
        for name in &member_names {
            if let Some(teammate) = team.members.get(name) {
                let has_all = required_capabilities
                    .iter()
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

            return best_agent
                .ok_or_else(|| AgentError::Communication("No available agents".to_string()));
        }

        // Fallback: no agent matches all capabilities, use first available.
        tracing::warn!(
            team = %team_name,
            required = ?required_capabilities,
            "No agent matches required capabilities, falling back to FirstAvailable"
        );

        member_names
            .first()
            .cloned()
            .ok_or_else(|| AgentError::Communication("No available agents".to_string()))
    }

    /// First-available assignment: pick the first member in the team.
    async fn assign_first_available(&self, team_name: &str) -> Result<String, AgentError> {
        let teams = self.teams.read().await;

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        team.members
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| AgentError::Communication("No available agents".to_string()))
    }

    // ── Self-Claim Task Assignment (Claude Code approach) ──────────────

    /// An agent explicitly claims a specific task by ID.
    ///
    /// The agent becomes the task owner and the task transitions to InProgress.
    /// This is the Claude Code model: idle agents claim tasks themselves rather
    /// than waiting for central assignment.
    ///
    /// When file persistence is configured, this also acquires an exclusive
    /// file lock on the task to prevent race conditions between agents.
    pub async fn self_claim_task(
        &self,
        team_name: &str,
        agent_name: &str,
        task_id: Uuid,
    ) -> Result<AgentTask, AgentError> {
        // Verify the agent is a member of the team
        {
            let teams = self.teams.read().await;
            let team = teams.get(team_name).ok_or_else(|| {
                AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
            })?;
            if !team.members.contains_key(agent_name) {
                return Err(AgentError::Communication(format!(
                    "Agent '{agent_name}' is not a member of team '{team_name}'"
                )));
            }
        }

        // Use file-based locking for claim conflict resolution if persistence is available
        if let Some(ref persist) = self.persistence {
            let claimed = persist.claim_task(team_name, &task_id.to_string(), agent_name)?;
            // Sync the in-memory task board with the persisted state
            self.task_board
                .assign_task(task_id, agent_name.to_string())
                .await?;
            self.task_board
                .update_task_status(task_id, TaskStatus::InProgress)
                .await?;
            let updated_task = self.task_board.get_task(task_id).await?;

            // Update the agent's assigned tasks
            {
                let teams = self.teams.read().await;
                if let Some(team) = teams.get(team_name) {
                    if let Some(agent) = team.members.get(agent_name) {
                        if let Err(e) = agent.assign_task(task_id).await {
                            tracing::warn!(
                                task_id = %task_id,
                                agent = %agent_name,
                                error = %e,
                                "Failed to assign task to agent after file-locked claim"
                            );
                        }
                    }
                }
            }

            if let Err(e) = self.event_sender.send(CoordinatorEvent::TaskAssigned {
                task_id,
                agent: agent_name.to_string(),
                subject: updated_task.subject.clone(),
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
                "Agent self-claimed task (file-locked)"
            );

            self.persist_team(team_name).await;

            // The returned claimed TaskFile may have slightly different timestamps
            // than the in-memory version, so use the task board's version.
            let _ = claimed; // Used for the file lock claim above
            return Ok(updated_task);
        }

        // Fallback: in-memory only claim (no persistence)
        // Use assign_task as an atomic claim — it will fail if already owned.
        let task = self.task_board.get_task(task_id).await?;
        if task.status != TaskStatus::Pending {
            return Err(AgentError::Task(crate::error::TaskError::InvalidTaskState(
                task_id,
            )));
        }
        if task.owner.is_some() {
            return Err(AgentError::Communication(format!(
                "Task {} already claimed by {:?}",
                task_id, task.owner
            )));
        }
        if !task.blocked_by.is_empty() {
            return Err(AgentError::Communication(format!(
                "Task {} is blocked by {:?}",
                task_id, task.blocked_by
            )));
        }

        // Assign and transition to InProgress — re-verify after assign to catch races
        self.task_board
            .assign_task(task_id, agent_name.to_string())
            .await?;
        let claimed = self.task_board.get_task(task_id).await?;
        if claimed.owner.as_deref() != Some(agent_name) {
            return Err(AgentError::Communication(format!(
                "Task {} was claimed by another agent ({:?})",
                task_id, claimed.owner
            )));
        }
        self.task_board
            .update_task_status(task_id, TaskStatus::InProgress)
            .await?;

        // Update the agent's assigned tasks
        {
            let teams = self.teams.read().await;
            if let Some(team) = teams.get(team_name) {
                if let Some(agent) = team.members.get(agent_name) {
                    if let Err(e) = agent.assign_task(task_id).await {
                        tracing::warn!(
                            task_id = %task_id,
                            agent = %agent_name,
                            error = %e,
                            "Failed to assign task to agent after in-memory claim"
                        );
                    }
                }
            }
        }

        let updated_task = self.task_board.get_task(task_id).await?;

        if let Err(e) = self.event_sender.send(CoordinatorEvent::TaskAssigned {
            task_id,
            agent: agent_name.to_string(),
            subject: updated_task.subject.clone(),
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
            let team = teams.get(team_name)?;
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

        let claimable = self
            .task_board
            .list_ready_tasks()
            .await
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

        // Collect summaries of recent peer DMs for this idle agent
        let peer_dm_summaries = self.collect_peer_dm_summaries(team_name, agent_name);
        if !peer_dm_summaries.is_empty() {
            tracing::info!(
                agent = %agent_name,
                team = %team_name,
                peer_dms = ?peer_dm_summaries,
                "Idle agent has recent peer direct messages"
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

    /// Collect summaries of recent peer-to-peer direct messages for a given agent.
    ///
    /// Scans the recent CoordinatorEvent stream for PeerDirectMessage events
    /// involving this agent (as sender or recipient) and returns their summaries.
    fn collect_peer_dm_summaries(&self, team_name: &str, agent_name: &str) -> Vec<String> {
        // Try to receive recent events without blocking the subscriber
        let mut rx = self.event_sender.subscribe();
        let mut summaries = Vec::new();

        // Drain buffered events that match our criteria
        while let Ok(event) = rx.try_recv() {
            if let CoordinatorEvent::PeerDirectMessage {
                team,
                from,
                to,
                summary,
            } = event
            {
                if team == team_name && (from == agent_name || to == agent_name) {
                    if let Some(s) = summary {
                        summaries.push(format!("{from}->{to}: {s}"));
                    } else {
                        summaries.push(format!("{from}->{to}: (no summary)"));
                    }
                }
            }
        }

        summaries
    }

    /// Get the task board
    pub fn task_board(&self) -> &TaskBoard {
        &self.task_board
    }

    /// Complete a task on the task board and fire the TeamTaskCompleted hook.
    ///
    /// This is the preferred way to mark a task as completed because it
    /// triggers the hook system for quality gates and persists the updated
    /// state to disk.
    pub async fn complete_task(
        &self,
        task_id: Uuid,
        team_name: &str,
        agent_name: &str,
    ) -> Result<(), AgentError> {
        // Fetch task details before completing (for hook subject)
        let subject = self
            .task_board
            .get_task(task_id)
            .await
            .map(|t| t.subject.clone())
            .unwrap_or_else(|_| task_id.to_string());

        self.task_board.complete_task(task_id).await?;

        // Persist the completed task to disk
        if let Ok(updated_task) = self.task_board.get_task(task_id).await {
            self.persist_task(team_name, &updated_task);
        }

        // Fire TeamTaskCompleted hook
        self.fire_hook(HookEvent::TeamTaskCompleted {
            task_id: task_id.to_string(),
            team_name: team_name.to_string(),
            agent_name: agent_name.to_string(),
            subject: subject.clone(),
        });

        // Fire TaskCompleted hook (Claude Code standard)
        self.fire_hook(HookEvent::TaskCompleted {
            task_id: task_id.to_string(),
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;
        let teammate = team.members.get(to_agent).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::AgentNotFound(to_agent.to_string()))
        })?;

        let request_id = Uuid::new_v4();
        let message = AgentMessage::protocol(
            from_agent.to_string(),
            to_agent.to_string(),
            ProtocolMessage::PlanApprovalRequest { request_id, plan },
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;
        let requester_agent = team.members.get(requester).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::AgentNotFound(requester.to_string()))
        })?;

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
            let feedback_text =
                feedback.unwrap_or_else(|| "No specific feedback provided".to_string());
            let message = AgentMessage::new_text(
                responder.to_string(),
                requester.to_string(),
                format!("Plan rejected. Feedback: {feedback_text}"),
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;
        Ok(self.team_manifest_internal(team).await)
    }

    /// Build manifest from a direct team reference (avoids re-locking).
    async fn team_manifest_internal(&self, team: &AgentTeam) -> TeamManifest {
        let members: Vec<AgentInfo> = team
            .members
            .iter()
            .map(|(name, m)| {
                let cfg = m.config();
                AgentInfo {
                    name: name.clone(),
                    agent_type: cfg.agent_type.clone(),
                    capabilities: cfg.capabilities.clone(),
                }
            })
            .collect();
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
        self.delegate_mode_flag
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Toggle delegate mode on or off.
    pub fn set_delegate_mode(&self, enabled: bool) {
        // Config is behind Arc<RwLock> in some paths, but here we access
        // the config field directly since it's on self.
        // Use interior mutability via a separate flag.
        self.delegate_mode_flag
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
    }

    /// Set the hook manager for firing team-related hooks.
    pub fn set_hook_manager(&mut self, manager: Arc<HookManager>) {
        self.hook_manager = Some(manager);
    }

    /// Load custom agent definitions from `.claude/agents/*.md` files.
    ///
    /// Scans both user-global (`~/.claude/agents/`) and project-local
    /// (`.claude/agents/`) directories. Project-local definitions override
    /// user-global ones with the same name.
    ///
    /// Returns the loaded definitions and stores them internally for use
    /// when spawning teammates with custom agent types.
    pub fn load_custom_agents(&mut self) -> Result<Vec<CustomAgentDef>, CustomAgentError> {
        let loader = CustomAgentLoader::new();
        let agents = loader.discover()?;

        let loaded: Vec<CustomAgentDef> = agents.values().cloned().collect();
        self.custom_agents = agents;

        tracing::info!(
            count = self.custom_agents.len(),
            "Loaded custom agent definitions"
        );

        Ok(loaded)
    }

    /// Get a loaded custom agent definition by name.
    pub fn get_custom_agent(&self, name: &str) -> Option<&CustomAgentDef> {
        self.custom_agents.get(name)
    }

    /// List all loaded custom agent names.
    pub fn list_custom_agents(&self) -> Vec<String> {
        self.custom_agents.keys().cloned().collect()
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
        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;
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

    /// Set the message history store for durable inter-agent message logging.
    pub fn set_message_history(&mut self, history: crate::message_history::MessageHistoryStore) {
        self.message_history = Some(history);
    }

    /// Get a reference to the message history store (if configured).
    pub fn message_history(&self) -> Option<&crate::message_history::MessageHistoryStore> {
        self.message_history.as_ref()
    }

    /// Record an outgoing message to history (if configured). Best-effort:
    /// failures are logged as warnings, not propagated.
    fn record_to_history(&self, team_name: &str, message: &AgentMessage) {
        let Some(ref store) = self.message_history else {
            return;
        };
        let (kind, preview) = match &message.content {
            MessageContent::Text(t) => (ContentKind::Text, t.clone()),
            MessageContent::Structured(v) => (
                ContentKind::Structured,
                serde_json::to_string(v).unwrap_or_default(),
            ),
            MessageContent::Protocol(p) => (ContentKind::Protocol, format!("{p:?}")),
        };
        let rec = crate::message_history::MessageRecord {
            message_id: message.id.to_string(),
            team: team_name.to_string(),
            from: message.from.clone(),
            to: message.to.clone(),
            content_preview: crate::message_history::MessageRecord::truncate_preview(&preview),
            content_kind: kind,
            priority: format!("{:?}", message.priority).to_lowercase(),
            timestamp: message.timestamp,
            revision: 0,
        };
        if let Err(e) = store.record(&rec) {
            tracing::warn!(team = %team_name, error = %e, "Failed to record message history");
        }
    }

    /// Load persisted teams and tasks from disk into memory.
    /// Call this once after coordinator creation, before use.
    ///
    /// Uses the dual-path persistence layer to discover teams stored under
    /// `~/.claude/` (preferred) or `~/.shannon/`.
    pub async fn load_from_disk(&self) -> Result<usize, AgentError> {
        let Some(ref persist) = self.persistence else {
            return Ok(0);
        };

        let all_teams = persist.load_all_teams()?;
        let mut loaded = 0;

        for (team_name, config_file) in all_teams {
            // Reconstruct teammates from config
            let mut members = HashMap::new();
            for (agent_name, teammate_config) in &config_file.members {
                let teammate = Teammate::new(agent_name.clone(), teammate_config.clone());
                teammate.set_team_name(team_name.clone());
                members.insert(agent_name.clone(), teammate);
            }

            // Load persisted tasks
            let task_files = persist.load_tasks(&team_name)?;
            let mut task_list = Vec::new();
            for tf in &task_files {
                if let Ok(task) = tf.to_agent_task() {
                    // Also add to the shared task board
                    let _ = self.task_board.add_task(task.clone()).await;
                    task_list.push(task);
                } else {
                    tracing::warn!("Skipping malformed task file: {}", tf.id);
                }
            }

            let member_count = members.len();
            let task_count = task_list.len();

            let team = AgentTeam {
                name: config_file.name.clone(),
                description: config_file.description.clone(),
                members,
                task_list,
                created_at: config_file.created_at.parse().unwrap_or(chrono::Utc::now()),
                assignment_index: config_file.assignment_index,
            };

            self.teams.write().await.insert(team_name.clone(), team);
            loaded += 1;

            tracing::info!(
                team = %team_name,
                members = member_count,
                tasks = task_count,
                "Loaded team from disk"
            );
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

        let members: HashMap<String, TeammateConfig> = team
            .members
            .iter()
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

    /// Persist a single task file to disk.
    fn persist_task(&self, team_name: &str, task: &AgentTask) {
        let Some(ref persist) = self.persistence else {
            return;
        };

        let task_file = crate::persistence::TaskFile::from_agent_task(task);
        if let Err(e) = persist.save_task(team_name, &task_file) {
            tracing::warn!(
                team = %team_name,
                task_id = %task.id,
                error = %e,
                "Failed to persist task file"
            );
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
                    if let Err(e) = agent.handle_message(message.clone()).await {
                        tracing::warn!(
                            agent = %_agent_name,
                            error = %e,
                            "Failed to deliver broadcast message to agent"
                        );
                    }
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
            return Err(AgentError::Coordination(CoordinationError::AgentNotFound(
                message.to,
            )));
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
                let claimable = ready_tasks.into_iter().find(|t| t.owner.is_none());

                if let Some(task) = claimable {
                    match task_board.assign_task(task.id, agent_name.clone()).await {
                        Ok(()) => {
                            // Update teammate's internal state so it won't be re-claimed
                            if let Err(e) = agent.assign_task(task.id).await {
                                tracing::warn!(
                                    agent = %agent_name,
                                    task_id = %task.id,
                                    error = %e,
                                    "Failed to update teammate state after auto-claim"
                                );
                            }
                            tracing::info!(
                                team = %team_name,
                                agent = %agent_name,
                                task_id = %task.id,
                                "Auto-claimed task for idle agent via heartbeat"
                            );
                            if let Err(e) = event_sender.send(CoordinatorEvent::TaskAutoClaimed {
                                task_id: task.id,
                                team: team_name.clone(),
                                agent: agent_name.clone(),
                            }) {
                                tracing::warn!(
                                    task_id = %task.id,
                                    agent = %agent_name,
                                    error = %e,
                                    "Failed to send TaskAutoClaimed event"
                                );
                            }
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

    /// Add a teammate to a team from a custom agent definition.
    ///
    /// Converts the `CustomAgentDef` to a `TeammateConfig` and delegates
    /// to [`add_teammate`]. If the agent definition specifies worktree
    /// isolation, the worktree is created automatically.
    pub async fn add_agent_from_def(
        &self,
        team_name: &str,
        def: &CustomAgentDef,
    ) -> Result<(), AgentError> {
        let config = def.to_teammate_config();
        self.add_teammate(team_name, def.name.clone(), config).await
    }

    /// Discover and cache agent definitions from `.claude/agents/` directories.
    ///
    /// Call this once during initialization. Discovered definitions can then
    /// be looked up by name via [`get_custom_agent`].
    pub async fn discover_custom_agents(&mut self) -> Result<(), AgentError> {
        let loader = CustomAgentLoader::new();
        let agents = loader
            .discover()
            .map_err(|e| AgentError::Worktree(format!("Failed to discover agents: {e}")))?;

        let count = agents.len();
        self.custom_agents.extend(agents);

        tracing::info!(count, "Discovered custom agent definitions");
        Ok(())
    }

    /// Disband a specific team: shutdown all agents, remove from memory, delete persisted files.
    pub async fn disband_team(&self, team_name: &str) -> Result<(), AgentError> {
        tracing::info!(team = %team_name, "Disbanding team");

        let mut teams = self.teams.write().await;
        let team = teams.get_mut(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

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
            if let Err(e) = teammate.handle_message(shutdown_msg).await {
                tracing::warn!(
                    team = %team_name,
                    agent = %agent_name,
                    error = %e,
                    "Failed to send shutdown message during disband"
                );
            }
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
                if let Err(e) = self
                    .task_board
                    .fail_task(task.id, "Team disbanded".to_string())
                    .await
                {
                    tracing::warn!(
                        task_id = %task.id,
                        agent = %member,
                        error = %e,
                        "Failed to fail task during team disband"
                    );
                }
            }
        }

        // Clean up process agents for this team
        if let Some(ref pm) = self.process_manager {
            for member in &member_names {
                if let Err(e) = pm
                    .graceful_shutdown_agent(member, Duration::from_secs(5))
                    .await
                {
                    tracing::warn!(
                        agent = %member,
                        error = %e,
                        "Failed to gracefully shutdown process agent during disband"
                    );
                }
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

                    match self
                        .add_task(
                            &team_name,
                            subject.to_string(),
                            description.to_string(),
                            priority,
                        )
                        .await
                    {
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
                            match self
                                .task_board
                                .update_task_status(task_id, new_status)
                                .await
                            {
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
                    Ok(task_id) => match self.task_board.get_task(task_id).await {
                        Ok(task) => serde_json::json!({
                            "id": task.id.to_string(),
                            "subject": task.subject,
                            "description": task.description,
                            "status": format!("{:?}", task.status),
                            "priority": format!("{:?}", task.priority),
                            "owner": task.owner,
                        }),
                        Err(e) => serde_json::json!({"error": e.to_string()}),
                    },
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
                let task_list: Vec<serde_json::Value> = tasks
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "id": t.id.to_string(),
                            "subject": t.subject,
                            "status": format!("{:?}", t.status),
                            "priority": format!("{:?}", t.priority),
                            "owner": t.owner,
                        })
                    })
                    .collect();
                serde_json::json!({"tasks": task_list})
            }
            "claim_task" => {
                let agent_name = params["agent_name"].as_str().unwrap_or_default();
                let task_id_str = params["task_id"].as_str().unwrap_or("");

                // Validate agent is a team member
                {
                    let teams = self.teams.read().await;
                    let is_member = teams
                        .values()
                        .any(|team| team.members.contains_key(agent_name));
                    if !is_member {
                        drop(teams);
                        return serde_json::json!({"error": format!("agent '{}' is not a team member", agent_name)});
                    }
                }

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
                        match self
                            .add_teammate(team_name, new_agent_name.to_string(), config)
                            .await
                        {
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

        // Signal delivery loop to stop
        let _ = self.delivery_cancel.send(true);

        // Cancel all tracked background tasks
        {
            let mut bg = self.background_tasks.write().await;
            for (key, handle) in bg.drain() {
                tracing::debug!("Aborting background task: {}", key);
                handle.abort();
            }
        }

        let teams = self.teams.write().await;

        // Send shutdown requests to all teammates
        for (team_name, team) in teams.iter() {
            tracing::debug!("Shutting down team '{}'", team_name);

            for (agent_name, teammate) in team.members.iter() {
                tracing::debug!(
                    "Shutting down agent '{}' in team '{}'",
                    agent_name,
                    team_name
                );

                // Send shutdown protocol message
                let shutdown_msg = AgentMessage::protocol(
                    "coordinator".to_string(),
                    agent_name.clone(),
                    ProtocolMessage::ShutdownRequest {
                        reason: "Coordinator shutting down".to_string(),
                    },
                );

                if let Err(e) = teammate.handle_message(shutdown_msg).await {
                    tracing::warn!(
                        team = %team_name,
                        agent = %agent_name,
                        error = %e,
                        "Failed to send shutdown message during coordinator shutdown"
                    );
                }
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
            // Collect session paths before cleanup for hook firing
            let sessions = manager.list_sessions().await;
            if let Err(e) = manager.cleanup_all().await {
                tracing::warn!(error = %e, "Failed to cleanup worktrees during shutdown");
            }
            // Fire WorktreeRemove hooks for each cleaned-up session
            for session in sessions {
                self.fire_hook(HookEvent::WorktreeRemove {
                    path: session.path.to_string_lossy().to_string(),
                });
            }
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
        let teams = self
            .teams
            .try_read()
            .map_err(|_| AgentError::Communication("Teams lock poisoned".to_string()))?;

        let team = teams.get(team_name).ok_or_else(|| {
            AgentError::Coordination(CoordinationError::TeamNotFound(team_name.to_string()))
        })?;

        let teammate = team.members.get(agent_name).cloned().ok_or_else(|| {
            AgentError::Coordination(CoordinationError::AgentNotFound(agent_name.to_string()))
        })?;

        let from = reply_to.to_string();
        let agent_name_owned = agent_name.to_string();
        let agent_type = teammate.config().agent_type.clone();
        let task_key = format!("{team_name}:{agent_name}");
        let message = AgentMessage::new_text(from.clone(), agent_name_owned.clone(), content);

        // Drop the teams lock before spawning the task
        drop(teams);

        let background_tasks = self.background_tasks.clone();
        let event_sender = self.event_sender.clone();
        let team_name_owned = team_name.to_string();
        let task_key_for_cleanup = task_key.clone();
        let hook_manager = self.hook_manager.clone();
        let agent_id_for_hook = agent_name_owned.clone();
        let agent_type_for_hook = agent_type.clone();

        // Fire SubagentStart hook before spawning the work
        if let Some(hm) = &hook_manager {
            let hm = hm.clone();
            let agent_id = agent_id_for_hook.clone();
            let at = agent_type_for_hook.clone();
            tokio::spawn(async move {
                let _ = hm
                    .run_hooks(&HookEvent::SubagentStart {
                        agent_id,
                        agent_type: at,
                    })
                    .await;
            });
        }

        let handle = tokio::spawn(async move {
            // Emit started event
            if let Err(e) = event_sender.send(CoordinatorEvent::AgentOutput {
                team: team_name_owned.clone(),
                agent: agent_name_owned.clone(),
                chunk: "[started]".to_string(),
            }) {
                tracing::debug!("Failed to send agent started event: {e}");
            }

            let result = if let Some(secs) = timeout_secs {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(secs),
                    teammate.handle_chat_message(message),
                )
                .await
                {
                    Ok(inner) => inner,
                    Err(_) => {
                        tracing::warn!(
                            agent = %agent_name_owned,
                            timeout_secs = secs,
                            "Background task timed out"
                        );
                        // Fire SubagentStop hook (timeout)
                        if let Some(hm) = &hook_manager {
                            let hm = hm.clone();
                            let aid = agent_name_owned.clone();
                            tokio::spawn(async move {
                                let _ = hm
                                    .run_hooks(&HookEvent::SubagentStop {
                                        agent_id: aid,
                                        result_summary: "timed out".to_string(),
                                    })
                                    .await;
                            });
                        }
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
                    // Fire SubagentStop hook (success)
                    if let Some(hm) = &hook_manager {
                        let hm = hm.clone();
                        let aid = agent_name_owned.clone();
                        let summary = output.clone();
                        tokio::spawn(async move {
                            let _ = hm
                                .run_hooks(&HookEvent::SubagentStop {
                                    agent_id: aid,
                                    result_summary: summary,
                                })
                                .await;
                        });
                    }
                    if let Err(e) = event_sender.send(CoordinatorEvent::AgentCompleted {
                        team: team_name_owned.clone(),
                        agent: agent_name_owned.clone(),
                        success: true,
                        output,
                    }) {
                        tracing::debug!("Failed to send agent completed event: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        agent = %agent_name_owned,
                        error = %e,
                        "Background task failed"
                    );
                    // Fire SubagentStop hook (failure)
                    if let Some(hm) = &hook_manager {
                        let hm = hm.clone();
                        let aid = agent_name_owned.clone();
                        let summary = e.to_string();
                        tokio::spawn(async move {
                            let _ = hm
                                .run_hooks(&HookEvent::SubagentStop {
                                    agent_id: aid,
                                    result_summary: summary,
                                })
                                .await;
                        });
                    }
                    if let Err(e) = event_sender.send(CoordinatorEvent::AgentCompleted {
                        team: team_name_owned.clone(),
                        agent: agent_name_owned.clone(),
                        success: false,
                        output: e.to_string(),
                    }) {
                        tracing::debug!("Failed to send agent failed event: {e}");
                    }
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
        let key = format!("{team_name}:{agent_name}");
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TaskError;

    #[test]
    fn coordinator_config_defaults() {
        let config = CoordinatorConfig::default();
        assert_eq!(config.max_team_size, 10);
        assert_eq!(config.message_buffer_size, 100);
        assert!(!config.enable_worktree_isolation);
        assert!(config.worktree_config.is_none());
        assert_eq!(config.heartbeat_interval_secs, 30);
        assert_eq!(config.assignment_strategy, AssignmentStrategy::SelfClaim);
        assert!(!config.delegate_mode);
        assert_eq!(config.agent_mode, AgentMode::InProcess);
    }

    #[test]
    fn assignment_strategy_serde() {
        let strategies = vec![
            AssignmentStrategy::RoundRobin,
            AssignmentStrategy::LeastLoaded,
            AssignmentStrategy::CapabilityBased,
            AssignmentStrategy::FirstAvailable,
            AssignmentStrategy::SelfClaim,
        ];
        let json = serde_json::to_string(&strategies).unwrap();
        let de: Vec<AssignmentStrategy> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, strategies);
    }

    #[test]
    fn agent_mode_serde() {
        let modes = vec![AgentMode::InProcess, AgentMode::Process];
        let json = serde_json::to_string(&modes).unwrap();
        let de: Vec<AgentMode> = serde_json::from_str(&json).unwrap();
        assert_eq!(de, modes);
    }

    #[test]
    fn agent_mode_default_is_inprocess() {
        assert_eq!(AgentMode::default(), AgentMode::InProcess);
    }

    #[test]
    fn agent_info_serde() {
        let info = AgentInfo {
            name: "worker-1".to_string(),
            agent_type: "coder".to_string(),
            capabilities: vec!["read".to_string(), "write".to_string()],
        };
        let json = serde_json::to_string(&info).unwrap();
        let de: AgentInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(de.name, "worker-1");
        assert_eq!(de.capabilities.len(), 2);
    }

    #[test]
    fn team_manifest_serde() {
        let manifest = TeamManifest {
            name: "alpha".to_string(),
            description: "test team".to_string(),
            members: vec![AgentInfo {
                name: "lead".to_string(),
                agent_type: "coordinator".to_string(),
                capabilities: vec![],
            }],
        };
        let json = serde_json::to_string(&manifest).unwrap();
        let de: TeamManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(de.name, "alpha");
        assert_eq!(de.members.len(), 1);
    }

    #[test]
    fn inbox_summary_default() {
        let summary = InboxSummary::default();
        assert_eq!(summary.total, 0);
        assert_eq!(summary.unread, 0);
        assert!(summary.senders.is_empty());
    }

    #[test]
    fn inbox_summary_serde() {
        let summary = InboxSummary {
            total: 5,
            unread: 2,
            senders: vec!["agent-a".to_string()],
        };
        let json = serde_json::to_string(&summary).unwrap();
        let de: InboxSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(de.total, 5);
        assert_eq!(de.unread, 2);
    }

    #[test]
    fn coordinator_config_custom_values() {
        let config = CoordinatorConfig {
            max_team_size: 5,
            message_buffer_size: 50,
            enable_worktree_isolation: true,
            worktree_config: None,
            heartbeat_interval_secs: 10,
            assignment_strategy: AssignmentStrategy::RoundRobin,
            delegate_mode: true,
            agent_mode: AgentMode::Process,
        };
        assert_eq!(config.max_team_size, 5);
        assert!(config.enable_worktree_isolation);
        assert!(config.delegate_mode);
        assert_eq!(config.agent_mode, AgentMode::Process);
    }

    #[test]
    fn coordination_error_messages() {
        let err = CoordinationError::TeamNotFound("my-team".to_string());
        assert!(err.to_string().contains("my-team"));

        let err = CoordinationError::AgentNotFound("agent-1".to_string());
        assert!(err.to_string().contains("agent-1"));

        let err = CoordinationError::AgentAlreadyMember("a".to_string(), "t".to_string());
        assert!(err.to_string().contains("a") && err.to_string().contains("t"));

        let err = CoordinationError::MaxTeamSizeExceeded(10);
        assert!(err.to_string().contains("10"));

        let err = CoordinationError::ShutdownInProgress;
        assert!(err.to_string().contains("shutdown"));
    }

    #[test]
    fn agent_error_from_coordination() {
        let coord_err = CoordinationError::TeamNotFound("x".to_string());
        let agent_err: AgentError = coord_err.into();
        assert!(agent_err.to_string().contains("coordination"));
    }

    #[test]
    fn task_error_messages() {
        let id = uuid::Uuid::new_v4();
        let err = TaskError::TaskNotFound(id);
        assert!(err.to_string().contains(&id.to_string()));

        let err = TaskError::TaskBlocked(id);
        assert!(err.to_string().contains("blocked"));

        let err = TaskError::NoAvailableAgents("search".to_string());
        assert!(err.to_string().contains("search"));
    }

    #[test]
    fn agent_error_from_task() {
        let id = uuid::Uuid::new_v4();
        let task_err = TaskError::TaskNotFound(id);
        let agent_err: AgentError = task_err.into();
        assert!(agent_err.to_string().contains("task"));
    }

    #[test]
    fn send_sync_types() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CoordinatorConfig>();
        assert_send_sync::<AgentInfo>();
        assert_send_sync::<TeamManifest>();
        assert_send_sync::<InboxSummary>();
        assert_send_sync::<AssignmentStrategy>();
        assert_send_sync::<AgentMode>();
    }

    // ── Hook event wiring tests ──────────────────────────────────────────

    #[test]
    fn task_created_hook_event_fields() {
        let event = HookEvent::TaskCreated {
            task_id: "t-123".to_string(),
            subject: "implement auth".to_string(),
            priority: "High".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::TaskCreated
        );
        assert_eq!(event.match_subject(), "implement auth");
        // Verify JSON roundtrip
        let json = serde_json::to_vec(&event).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn task_completed_hook_event_fields() {
        let event = HookEvent::TaskCompleted {
            task_id: "t-456".to_string(),
            subject: "fix bug".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::TaskCompleted
        );
        assert_eq!(event.match_subject(), "fix bug");
        let json = serde_json::to_vec(&event).unwrap();
        assert!(!json.is_empty());
    }

    #[test]
    fn subagent_start_hook_event_fields() {
        let event = HookEvent::SubagentStart {
            agent_id: "agent-007".to_string(),
            agent_type: "general-purpose".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::SubagentStart
        );
        assert_eq!(event.match_subject(), "agent-007");
    }

    #[test]
    fn subagent_stop_hook_event_fields() {
        let event = HookEvent::SubagentStop {
            agent_id: "agent-007".to_string(),
            result_summary: "completed task".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::SubagentStop
        );
        assert_eq!(event.match_subject(), "agent-007");
    }

    #[test]
    fn worktree_create_hook_event_fields() {
        let event = HookEvent::WorktreeCreate {
            path: "/tmp/worktree-1".to_string(),
            branch: "worktree/agent-1".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::WorktreeCreate
        );
        assert_eq!(event.match_subject(), "/tmp/worktree-1");
    }

    #[test]
    fn worktree_remove_hook_event_fields() {
        let event = HookEvent::WorktreeRemove {
            path: "/tmp/worktree-1".to_string(),
        };
        assert_eq!(
            event.event_type(),
            shannon_core::hooks::HookEventType::WorktreeRemove
        );
        assert_eq!(event.match_subject(), "/tmp/worktree-1");
    }

    #[tokio::test]
    async fn coordinator_fire_hook_with_no_hook_manager_does_not_panic() {
        // Create a coordinator without a hook manager and verify fire_hook doesn't panic.
        // We test this indirectly by creating tasks which fire hooks internally.
        let config = CoordinatorConfig::default();
        let coordinator = AgentCoordinator::new(config).await.unwrap();
        // No hook_manager set — fire_hook should be a no-op
        coordinator.fire_hook(HookEvent::TaskCreated {
            task_id: "test".to_string(),
            subject: "test".to_string(),
            priority: "Low".to_string(),
        });
        // Give spawned task (if any) time to settle
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn coordinator_set_hook_manager_accepts_manager() {
        let config = CoordinatorConfig::default();
        let mut coordinator = AgentCoordinator::new(config).await.unwrap();
        let hm = Arc::new(HookManager::new());
        coordinator.set_hook_manager(hm);
        // Hook manager is now set — fire_hook should work without panic
        coordinator.fire_hook(HookEvent::TaskCreated {
            task_id: "test2".to_string(),
            subject: "test2".to_string(),
            priority: "Medium".to_string(),
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn coordinator_record_to_history_persists_message() {
        // Wire a MessageHistoryStore into the coordinator and verify
        // record_to_history appends to the per-team log.
        let tmp = tempfile::tempdir().unwrap();
        let store =
            crate::message_history::MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let config = CoordinatorConfig::default();
        let mut coordinator = AgentCoordinator::new(config).await.unwrap();
        coordinator.set_message_history(store);

        let id = Uuid::new_v4();
        let msg = crate::message::AgentMessage {
            id,
            from: "alice".into(),
            to: "bob".into(),
            message_type: crate::message::MessageType::Chat,
            priority: crate::message::MessagePriority::Normal,
            content: MessageContent::Text("hello world".into()),
            timestamp: chrono::Utc::now(),
        };
        coordinator.record_to_history("alpha", &msg);

        let history = coordinator.message_history().unwrap();
        let list = history.list_by_team("alpha", 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].from, "alice");
        assert_eq!(list[0].to, "bob");
        assert_eq!(list[0].content_preview, "hello world");
        assert_eq!(
            list[0].content_kind,
            crate::message_history::ContentKind::Text
        );
    }

    #[tokio::test]
    async fn coordinator_record_to_history_noop_without_store() {
        // No history configured — record_to_history should silently no-op.
        let config = CoordinatorConfig::default();
        let coordinator = AgentCoordinator::new(config).await.unwrap();
        let msg = crate::message::AgentMessage {
            id: Uuid::new_v4(),
            from: "alice".into(),
            to: "bob".into(),
            message_type: crate::message::MessageType::Chat,
            priority: crate::message::MessagePriority::Normal,
            content: MessageContent::Text("hello".into()),
            timestamp: chrono::Utc::now(),
        };
        coordinator.record_to_history("alpha", &msg);
        assert!(coordinator.message_history().is_none());
    }

    #[tokio::test]
    async fn coordinator_record_to_history_structured_content() {
        let tmp = tempfile::tempdir().unwrap();
        let store =
            crate::message_history::MessageHistoryStore::with_base(tmp.path().to_path_buf());

        let config = CoordinatorConfig::default();
        let mut coordinator = AgentCoordinator::new(config).await.unwrap();
        coordinator.set_message_history(store);

        let msg = crate::message::AgentMessage {
            id: Uuid::new_v4(),
            from: "carol".into(),
            to: "dave".into(),
            message_type: crate::message::MessageType::Chat,
            priority: crate::message::MessagePriority::High,
            content: MessageContent::Structured(serde_json::json!({"key": "value"})),
            timestamp: chrono::Utc::now(),
        };
        coordinator.record_to_history("beta", &msg);

        let history = coordinator.message_history().unwrap();
        let list = history.list_by_team("beta", 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].content_kind,
            crate::message_history::ContentKind::Structured
        );
        assert_eq!(list[0].priority, "high");
        assert!(list[0].content_preview.contains("key"));
    }
}
