//! # Shannon Multi-Agent Coordination System
//!
//! This crate provides a framework for coordinating multiple AI agents working together
//! on complex tasks. It supports team creation, task delegation, message passing,
//! and git worktree isolation for parallel development workflows.

mod coordinator;
mod teammate;
mod task_board;
mod worktree;
mod message;
mod task;
mod error;
mod sub_agent;
mod multi_agent;
mod executor;
mod summary;
mod context;
mod task_tools;
mod persistence;
mod agent_defs;
mod tmux;

pub use coordinator::{AgentCoordinator, CoordinatorConfig, AssignmentStrategy, CoordinatorEvent, AgentInfo, TeamManifest, InboxSummary};
pub use teammate::{Teammate, TeammateConfig, TeammateStatus, TeammateState};
pub use task_board::{TaskBoard, TaskAssignment, TaskBoardEvent, TaskBoardSummary};
pub use worktree::{WorktreeManager, WorktreeConfig, WorktreeSession, WorktreeStatus, ExitAction,
    EnterWorktreeTool, ExitWorktreeTool,
    EnterWorktreeToolInput, ExitWorktreeToolInput,
    get_active_worktree};
pub use message::{AgentMessage, MessagePriority, MessageType, MessageContent, ProtocolMessage};
pub use task::{AgentTask, TaskStatus, TaskDependency, TaskPriority, DependencyType};
pub use error::{AgentError, CoordinationError, TaskError};
pub use sub_agent::{
    AgentConfig, AgentStatus, SubAgent, SubAgentRegistry,
    AgentSpawnTool, AgentSpawnInput,
    SendMessageTool, SendMessageInput,
    TeamCreateTool, TeamCreateInput,
};
pub use multi_agent::{
    MultiAgentConfig, MultiAgentSpawner, MultiAgentResult,
    AgentResult as MultiAgentTaskResult, AgentResultStatus,
    AgentConfig as SpawnAgentConfig,
    DependencyError,
};
pub use summary::{
    AgentExecutionSummary, SummaryStatus, SummaryGenerator, SuccessMetrics,
};
pub use context::{TeamContext, teams_enabled, TEAMS_ENV_VAR};
pub use task_tools::{
    TeamTaskCreateTool, TeamTaskUpdateTool, TeamTaskListTool,
    TeamTaskClaimTool, TeamNotifyIdleTool,
};
pub use persistence::{
    FilePersistence, TeamConfigFile, TaskFile, InboxMessage,
};
pub use agent_defs::{
    AgentDefinition, AgentDefinitionRegistry, AgentDefError,
};
pub use executor::{
    AgentExecutor, LlmAgentExecutor, MockAgentExecutor, shared_executor,
    ChatTurn,
};
pub use tmux::TmuxManager;

/// Version information for the agents crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result type alias for agent operations
pub type AgentResult<T> = Result<T, AgentError>;
