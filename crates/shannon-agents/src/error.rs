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

    /// Configuration or feature-flag errors
    #[error("configuration error: {0}")]
    Configuration(String),

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_error_coordination_display() {
        let err = AgentError::Coordination(CoordinationError::TeamNotFound("alpha".into()));
        assert!(err.to_string().contains("alpha"));
        assert!(err.to_string().contains("coordination"));
    }

    #[test]
    fn agent_error_task_display() {
        let id = uuid::Uuid::new_v4();
        let err = AgentError::Task(TaskError::TaskNotFound(id));
        let msg = err.to_string();
        assert!(msg.contains(&id.to_string()));
    }

    #[test]
    fn agent_error_worktree_display() {
        let err = AgentError::Worktree("path conflict".into());
        assert!(err.to_string().contains("path conflict"));
    }

    #[test]
    fn agent_error_communication_display() {
        let err = AgentError::Communication("channel closed".into());
        assert!(err.to_string().contains("channel closed"));
    }

    #[test]
    fn agent_error_configuration_display() {
        let err = AgentError::Configuration("bad cfg".into());
        assert!(err.to_string().contains("bad cfg"));
    }

    #[test]
    fn agent_error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err: AgentError = io_err.into();
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn agent_error_from_serde_json() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let err: AgentError = json_err.into();
        assert!(err.to_string().contains("serialization"));
    }

    #[test]
    fn coordination_error_variants() {
        assert!(CoordinationError::TeamNotFound("t".into()).to_string().contains("t"));
        assert!(CoordinationError::AgentNotFound("a".into()).to_string().contains("a"));
        assert!(CoordinationError::AgentAlreadyMember("a".into(), "t".into()).to_string().contains("a"));
        assert!(CoordinationError::InvalidConfiguration("bad".into()).to_string().contains("bad"));
        assert!(CoordinationError::ShutdownInProgress.to_string().contains("shutdown"));
        assert!(CoordinationError::MaxTeamSizeExceeded(10).to_string().contains("10"));
    }

    #[test]
    fn task_error_variants() {
        let id = uuid::Uuid::new_v4();
        assert!(TaskError::TaskNotFound(id).to_string().contains(&id.to_string()));
        assert!(TaskError::TaskBlocked(id).to_string().contains("blocked"));
        assert!(TaskError::InvalidTaskState(id).to_string().contains("invalid state"));
        assert!(TaskError::CircularDependency(id).to_string().contains("circular"));
        assert!(TaskError::TaskFailed(id, "timeout".into()).to_string().contains("timeout"));
        assert!(TaskError::NoAvailableAgents("task".into()).to_string().contains("task"));
    }

    #[test]
    fn agent_error_from_coordination() {
        let coord_err = CoordinationError::ShutdownInProgress;
        let err: AgentError = coord_err.into();
        assert!(matches!(err, AgentError::Coordination(CoordinationError::ShutdownInProgress)));
    }

    #[test]
    fn agent_error_from_task() {
        let id = uuid::Uuid::new_v4();
        let task_err = TaskError::TaskNotFound(id);
        let err: AgentError = task_err.into();
        assert!(matches!(err, AgentError::Task(TaskError::TaskNotFound(_))));
    }

    #[test]
    fn send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<AgentError>();
        assert_send_sync::<CoordinationError>();
        assert_send_sync::<TaskError>();
    }
}
