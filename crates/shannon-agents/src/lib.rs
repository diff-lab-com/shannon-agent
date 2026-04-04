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
mod summary;

pub use coordinator::{AgentCoordinator, CoordinatorConfig, AssignmentStrategy, CoordinatorEvent};
pub use teammate::{Teammate, TeammateConfig, TeammateStatus, TeammateState};
pub use task_board::{TaskBoard, TaskAssignment, TaskBoardEvent, TaskBoardSummary};
pub use worktree::{WorktreeManager, WorktreeConfig, WorktreeSession, WorktreeStatus, ExitAction,
    EnterWorktreeTool, ExitWorktreeTool,
    EnterWorktreeToolInput, ExitWorktreeToolInput};
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

/// Version information for the agents crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result type alias for agent operations
pub type AgentResult<T> = Result<T, AgentError>;
