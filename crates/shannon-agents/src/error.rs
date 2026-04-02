//! Error types for the agent coordination system

use thiserror::Error;

/// Main error type for agent operations
#[derive(Error, Debug)]
pub enum AgentError {
    /// Errors related to agent coordination
    #[error("coordination error: {0}")]
    Coordination(#[from] CoordinationError),

    /// Errors related to task management
    #[error("task error: {0}")]
    Task(#[from] TaskError),

    /// Worktree operation errors
    #[error("worktree error: {0}")]
    Worktree(String),

    /// Communication errors between agents
    #[error("communication error: {0}")]
    Communication(String),

    /// I/O errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization errors
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Errors specific to agent coordination
#[derive(Error, Debug)]
pub enum CoordinationError {
    #[error("team '{0}' not found")]
    TeamNotFound(String),

    #[error("agent '{0}' not found in team")]
    AgentNotFound(String),

    #[error("agent '{0}' is already a member of team '{1}'")]
    AgentAlreadyMember(String, String),

    #[error("invalid team configuration: {0}")]
    InvalidConfiguration(String),

    #[error("coordination shutdown in progress")]
    ShutdownInProgress,

    #[error("maximum team size ({0}) exceeded")]
    MaxTeamSizeExceeded(usize),
}

/// Errors specific to task management
#[derive(Error, Debug)]
pub enum TaskError {
    #[error("task '{0}' not found")]
    TaskNotFound(uuid::Uuid),

    #[error("task '{0}' is blocked by dependencies")]
    TaskBlocked(uuid::Uuid),

    #[error("task '{0}' cannot be assigned (invalid state)")]
    InvalidTaskState(uuid::Uuid),

    #[error("circular dependency detected involving task '{0}'")]
    CircularDependency(uuid::Uuid),

    #[error("task '{0}' failed: {1}")]
    TaskFailed(uuid::Uuid, String),

    #[error("no available agents for task '{0}'")]
    NoAvailableAgents(String),
}
