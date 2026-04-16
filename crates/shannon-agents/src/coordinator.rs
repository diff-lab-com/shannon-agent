//! Agent coordinator for managing multi-agent teams

use crate::{
    error::{AgentError, CoordinationError},
    message::{AgentMessage, MessageContent, MessageType, ProtocolMessage},
    persistence::{FilePersistence, InboxMessage, TeamConfigFile},
    task::{AgentTask, TaskPriority, TaskStatus},
    teammate::{Teammate, TeammateConfig, TeammateStatus},
    worktree::{WorktreeConfig, WorktreeManager},
    TaskBoard,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock, broadcast};
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

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            max_team_size: 10,
            message_buffer_size: 100,
            enable_worktree_isolation: false,
            worktree_config: None,
            heartbeat_interval_secs: 30,
            assignment_strategy: AssignmentStrategy::FirstAvailable,
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
        let heartbeat_interval = config.heartbeat_interval_secs;
        let heartbeat_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(heartbeat_interval));
            loop {
                interval.tick().await;
                Self::send_heartbeats(&teams_heartbeat, &event_heartbeat).await;
            }
        });

        Ok(Self {
            config,
            teams,
            worktree_manager,
            task_board,
            message_sender,
            event_sender,
            _message_receiver: Arc::new(message_handle),
            _heartbeat_handle: Arc::new(heartbeat_handle),
            persistence: None,
        })
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

        let teammate = Teammate::new(agent_name.clone(), config);
        team.members.insert(agent_name.clone(), teammate);

        drop(teams);

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
        let task = AgentTask::new(subject, description, priority);
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

        Ok(task_id)
    }

    /// Assign a task to an agent
    pub async fn assign_task(
        &self,
        team_name: &str,
        task_id: Uuid,
    ) -> Result<String, AgentError> {
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

    /// Find the next claimable task for an agent (lowest-ID unblocked, pending, unowned).
    ///
    /// This implements the Claude Code priority rule: prefer tasks in ID order
    /// so that earlier-created tasks are picked first.
    pub async fn find_next_claimable_task(&self, agent_name: &str) -> Option<AgentTask> {
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
            "Found claimable task for agent"
        );

        Some(task)
    }

    /// Convenience: find and claim the next available task for an agent.
    ///
    /// Returns the claimed task, or None if no tasks are available.
    pub async fn claim_next_task(
        &self,
        team_name: &str,
        agent_name: &str,
    ) -> Result<Option<AgentTask>, AgentError> {
        let Some(task) = self.find_next_claimable_task(agent_name).await else {
            return Ok(None);
        };

        let claimed = self.self_claim_task(team_name, agent_name, task.id).await?;
        Ok(Some(claimed))
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

        Ok(claimable)
    }

    /// Get the task board
    pub fn task_board(&self) -> &TaskBoard {
        &self.task_board
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

        let members: Vec<AgentInfo> = team.members.iter().map(|(name, m)| {
            let cfg = m.config();
            AgentInfo {
                name: name.clone(),
                agent_type: cfg.agent_type.clone(),
                capabilities: cfg.capabilities.clone(),
            }
        }).collect();

        Ok(TeamManifest {
            name: team.name.clone(),
            description: team.description.clone(),
            members,
        })
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
}
