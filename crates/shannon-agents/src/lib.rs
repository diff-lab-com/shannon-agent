//! # Shannon Multi-Agent Coordination System
//!
//! This crate provides a framework for coordinating multiple AI agents working together
//! on complex tasks. It supports team creation, task delegation, message passing,
//! and git worktree isolation for parallel development workflows.

pub mod agent_defs;
mod context;
mod coordinator;
pub mod custom_agent;
mod error;
mod executor;
mod isolation;
mod message;
mod multi_agent;
mod persistence;
mod process_manager;
mod protocol;
mod remote_tools;
mod sub_agent;
mod summary;
mod task;
mod task_board;
mod task_tools;
mod teammate;
mod tmux;
mod worktree;

pub use agent_defs::{AgentDefError, AgentDefinition, AgentDefinitionRegistry};
pub use context::{TEAMS_ENV_VAR, TeamContext, teams_enabled};
pub use coordinator::{
    AgentCoordinator, AgentInfo, AgentMode, AssignmentStrategy, CoordinatorConfig,
    CoordinatorEvent, InboxSummary, TeamManifest,
};
pub use custom_agent::{CustomAgentDef, CustomAgentError, CustomAgentLoader};
pub use error::{AgentError, CoordinationError, TaskError};
pub use executor::{AgentExecutor, ChatTurn, LlmAgentExecutor, MockAgentExecutor, shared_executor};
pub use isolation::{
    ContextMessage, ContextRole, IsolatedContext, IsolationConfig, SubagentSummary,
};
pub use message::{AgentMessage, MessageContent, MessagePriority, MessageType, ProtocolMessage};
pub use multi_agent::{
    AgentConfig as SpawnAgentConfig, AgentResult as MultiAgentTaskResult, AgentResultStatus,
    DependencyError, MultiAgentConfig, MultiAgentResult, MultiAgentSpawner,
};
pub use persistence::{FilePersistence, InboxMessage, TaskFile, TeamConfigFile};
pub use process_manager::{
    AgentEvent, AgentProcessConfig, AgentProcessError, AgentProcessManager, AgentProcessStatus,
    HealthCheckConfig,
};
pub use protocol::{
    AgentIdleParams, AgentReadyParams, ClaimTaskParams, ClaimTaskResult, ExecuteTaskParams,
    JsonRpcError, JsonRpcId, JsonRpcMessage, ListTasksParams, ListTasksResult, SendMessageParams,
    ShutdownParams, TaskCompleteParams, TaskProgressParams, TaskSummary, frame_message,
    parse_message,
};
pub use remote_tools::{
    CoordinatorChannel, RemoteAddAgentTool, RemoteDisbandTeamTool, RemoteSendMessageTool,
    RemoteTeamManifestTool, RemoteTeamNotifyIdleTool, RemoteTeamTaskClaimTool,
    RemoteTeamTaskCreateTool, RemoteTeamTaskGetTool, RemoteTeamTaskListTool,
    RemoteTeamTaskUpdateTool,
};
pub use sub_agent::{
    AgentConfig, AgentSpawnInput, AgentSpawnTool, AgentStatus, SendMessageInput, SendMessageTool,
    SubAgent, SubAgentRegistry, TeamCreateInput, TeamCreateTool,
};
pub use summary::{AgentExecutionSummary, SuccessMetrics, SummaryGenerator, SummaryStatus};
pub use task::{AgentTask, DependencyType, TaskDependency, TaskPriority, TaskStatus};
pub use task_board::{TaskAssignment, TaskBoard, TaskBoardEvent, TaskBoardSummary};
pub use task_tools::{
    TeamNotifyIdleTool, TeamTaskClaimTool, TeamTaskCreateTool, TeamTaskListTool, TeamTaskUpdateTool,
};
pub use teammate::{Teammate, TeammateConfig, TeammateState, TeammateStatus};
pub use tmux::TmuxManager;
pub use worktree::{
    EnterWorktreeTool, EnterWorktreeToolInput, ExitAction, ExitWorktreeTool, ExitWorktreeToolInput,
    WorktreeConfig, WorktreeManager, WorktreeSession, WorktreeStatus, get_active_worktree,
};

/// Version information for the agents crate
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result type alias for agent operations
pub type AgentResult<T> = Result<T, AgentError>;
