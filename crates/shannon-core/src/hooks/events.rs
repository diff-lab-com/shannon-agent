//! Hook event types and their associated data.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The type of hook event being triggered
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEventType {
    /// Before a tool is executed
    PreToolUse,
    /// After a tool completes
    PostToolUse,
    /// When a session begins
    SessionStart,
    /// When a session ends
    SessionEnd,
    /// When a notification is emitted
    Notification,
    /// When the user submits a prompt
    UserPromptSubmit,
    /// When a team task is created (before committing)
    TeamTaskCreated,
    /// When a team task is marked completed (before committing)
    TeamTaskCompleted,
    /// When a teammate goes idle
    TeammateIdle,
    /// Before context compaction
    PreCompact,
    /// When a subagent is spawned
    SubagentStart,
    /// When a subagent finishes
    SubagentStop,
    /// When a tool permission is denied
    PermissionDenied,
    /// When the model stops generating
    Stop,
    /// After a tool fails with an error
    PostToolUseFailure,
    /// After context compaction completes
    PostCompact,
    /// When the model stops due to an error
    StopFailure,
    /// When a file is modified on disk
    FileChanged,
    /// When the working directory changes
    CwdChanged,
    /// When a permission is requested (before user prompt)
    PermissionRequest,
    /// After user prompt is expanded (template variables resolved)
    UserPromptExpansion,
    /// After a batch of tools completes
    PostToolBatch,
    /// When configuration changes
    ConfigChange,
    /// After CLAUDE.md / instructions are loaded
    InstructionsLoaded,
    /// When a worktree is created
    WorktreeCreate,
    /// When a worktree is removed
    WorktreeRemove,
    /// When an interactive elicitation is triggered
    Elicitation,
    /// When an elicitation result is received
    ElicitationResult,
    /// When a task is created (Claude Code standard name)
    TaskCreated,
    /// When a task is completed (Claude Code standard name)
    TaskCompleted,
}

impl HookEventType {
    /// Parse a string into a HookEventType
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s {
            "PreToolUse" => Some(Self::PreToolUse),
            "PostToolUse" => Some(Self::PostToolUse),
            "SessionStart" => Some(Self::SessionStart),
            "SessionEnd" => Some(Self::SessionEnd),
            "Notification" => Some(Self::Notification),
            "UserPromptSubmit" => Some(Self::UserPromptSubmit),
            "TeamTaskCreated" => Some(Self::TeamTaskCreated),
            "TeamTaskCompleted" => Some(Self::TeamTaskCompleted),
            "TeammateIdle" => Some(Self::TeammateIdle),
            "PreCompact" => Some(Self::PreCompact),
            "SubagentStart" => Some(Self::SubagentStart),
            "SubagentStop" => Some(Self::SubagentStop),
            "PermissionDenied" => Some(Self::PermissionDenied),
            "Stop" => Some(Self::Stop),
            "PostToolUseFailure" => Some(Self::PostToolUseFailure),
            "PostCompact" => Some(Self::PostCompact),
            "StopFailure" => Some(Self::StopFailure),
            "FileChanged" => Some(Self::FileChanged),
            "CwdChanged" => Some(Self::CwdChanged),
            "PermissionRequest" => Some(Self::PermissionRequest),
            "UserPromptExpansion" => Some(Self::UserPromptExpansion),
            "PostToolBatch" => Some(Self::PostToolBatch),
            "ConfigChange" => Some(Self::ConfigChange),
            "InstructionsLoaded" => Some(Self::InstructionsLoaded),
            "WorktreeCreate" => Some(Self::WorktreeCreate),
            "WorktreeRemove" => Some(Self::WorktreeRemove),
            "Elicitation" => Some(Self::Elicitation),
            "ElicitationResult" => Some(Self::ElicitationResult),
            "TaskCreated" => Some(Self::TaskCreated),
            "TaskCompleted" => Some(Self::TaskCompleted),
            _ => None,
        }
    }
}

impl std::fmt::Display for HookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PreToolUse => write!(f, "PreToolUse"),
            Self::PostToolUse => write!(f, "PostToolUse"),
            Self::SessionStart => write!(f, "SessionStart"),
            Self::SessionEnd => write!(f, "SessionEnd"),
            Self::Notification => write!(f, "Notification"),
            Self::UserPromptSubmit => write!(f, "UserPromptSubmit"),
            Self::TeamTaskCreated => write!(f, "TeamTaskCreated"),
            Self::TeamTaskCompleted => write!(f, "TeamTaskCompleted"),
            Self::TeammateIdle => write!(f, "TeammateIdle"),
            Self::PreCompact => write!(f, "PreCompact"),
            Self::SubagentStart => write!(f, "SubagentStart"),
            Self::SubagentStop => write!(f, "SubagentStop"),
            Self::PermissionDenied => write!(f, "PermissionDenied"),
            Self::Stop => write!(f, "Stop"),
            Self::PostToolUseFailure => write!(f, "PostToolUseFailure"),
            Self::PostCompact => write!(f, "PostCompact"),
            Self::StopFailure => write!(f, "StopFailure"),
            Self::FileChanged => write!(f, "FileChanged"),
            Self::CwdChanged => write!(f, "CwdChanged"),
            Self::PermissionRequest => write!(f, "PermissionRequest"),
            Self::UserPromptExpansion => write!(f, "UserPromptExpansion"),
            Self::PostToolBatch => write!(f, "PostToolBatch"),
            Self::ConfigChange => write!(f, "ConfigChange"),
            Self::InstructionsLoaded => write!(f, "InstructionsLoaded"),
            Self::WorktreeCreate => write!(f, "WorktreeCreate"),
            Self::WorktreeRemove => write!(f, "WorktreeRemove"),
            Self::Elicitation => write!(f, "Elicitation"),
            Self::ElicitationResult => write!(f, "ElicitationResult"),
            Self::TaskCreated => write!(f, "TaskCreated"),
            Self::TaskCompleted => write!(f, "TaskCompleted"),
        }
    }
}

/// A concrete hook event with its associated data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum HookEvent {
    /// Before a tool is executed
    PreToolUse {
        /// Name of the tool about to be used
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
    },
    /// After a tool completes
    PostToolUse {
        /// Name of the tool that was used
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
        /// Output from the tool
        output: Value,
    },
    /// When a session begins
    SessionStart {
        /// Unique session identifier
        session_id: String,
    },
    /// When a session ends
    SessionEnd {
        /// Unique session identifier
        session_id: String,
    },
    /// When a notification is emitted
    Notification {
        /// Notification message content
        message: String,
    },
    /// When the user submits a prompt
    UserPromptSubmit {
        /// The user's prompt text
        prompt: String,
    },
    /// When a team task is created (before committing).
    /// Exit code 2 from the hook = rollback (delete the task).
    TeamTaskCreated {
        /// The task ID
        task_id: String,
        /// The team name
        team_name: String,
        /// The agent that created the task (if known)
        agent_name: Option<String>,
        /// Brief task subject
        subject: String,
        /// Task priority
        priority: String,
    },
    /// When a team task is marked completed (before committing).
    /// Exit code 2 from the hook = prevent completion (revert to in_progress).
    TeamTaskCompleted {
        /// The task ID
        task_id: String,
        /// The team name
        team_name: String,
        /// The agent that completed the task
        agent_name: String,
        /// Brief task subject
        subject: String,
    },
    /// When a teammate goes idle.
    /// Exit code 2 from the hook = send feedback and keep working.
    TeammateIdle {
        /// The team name
        team_name: String,
        /// The agent that went idle
        agent_name: String,
        /// Number of remaining available tasks
        available_tasks: usize,
    },
    /// Before context compaction
    PreCompact {
        /// Number of messages in the conversation
        messages_count: usize,
        /// Estimated token usage
        estimated_tokens: usize,
    },
    /// When a subagent is spawned
    SubagentStart {
        /// Unique agent identifier
        agent_id: String,
        /// Type of agent (e.g. "Explore", "general-purpose")
        agent_type: String,
    },
    /// When a subagent finishes
    SubagentStop {
        /// Unique agent identifier
        agent_id: String,
        /// Brief summary of the result
        result_summary: String,
    },
    /// When a tool permission is denied
    PermissionDenied {
        /// Name of the tool
        tool_name: String,
        /// Tool input that was denied
        input: Value,
        /// How many times the user has retried
        retry_count: usize,
    },
    /// When the model stops generating
    Stop {
        /// Number of tool calls made in this turn
        tool_calls_count: usize,
        /// Whether the model should continue (exit code 2 = force continue)
        should_continue: bool,
    },
    /// After a tool fails with an error
    PostToolUseFailure {
        /// Name of the tool that failed
        tool_name: String,
        /// Input/arguments for the tool
        input: Value,
        /// Error message from the tool
        error: String,
    },
    /// After context compaction completes
    PostCompact {
        /// Number of messages before compaction
        messages_before: usize,
        /// Number of messages after compaction
        messages_after: usize,
        /// Estimated tokens freed
        tokens_freed: usize,
    },
    /// When the model stops due to an error
    StopFailure {
        /// Error message that caused the stop
        error: String,
    },
    /// When a file is modified on disk
    FileChanged {
        /// Path to the changed file
        path: String,
        /// Type of change (create, modify, delete)
        change_type: String,
    },
    /// When the working directory changes
    CwdChanged {
        /// Previous working directory
        old_cwd: String,
        /// New working directory
        new_cwd: String,
    },
    /// When a permission is requested (before user prompt)
    PermissionRequest {
        /// Name of the tool requesting permission
        tool_name: String,
        /// Description of what the tool will do
        description: String,
    },
    /// After user prompt is expanded (template variables resolved)
    UserPromptExpansion {
        /// The expanded prompt text
        expanded_prompt: String,
        /// The original prompt before expansion
        original_prompt: String,
    },
    /// After a batch of tools completes
    PostToolBatch {
        /// Tool names in the batch
        tool_names: Vec<String>,
        /// Number of successful executions
        success_count: usize,
        /// Number of failed executions
        failure_count: usize,
    },
    /// When configuration changes
    ConfigChange {
        /// Path to the changed config file
        config_path: String,
        /// Type of change (created, modified, deleted)
        change_type: String,
    },
    /// After CLAUDE.md / instructions are loaded
    InstructionsLoaded {
        /// Number of instruction files loaded
        files_count: usize,
        /// Total size in bytes
        total_bytes: usize,
    },
    /// When a worktree is created
    WorktreeCreate {
        /// Path to the new worktree
        path: String,
        /// Branch name for the worktree
        branch: String,
    },
    /// When a worktree is removed
    WorktreeRemove {
        /// Path to the removed worktree
        path: String,
    },
    /// When an interactive elicitation is triggered
    Elicitation {
        /// The question being asked
        question: String,
        /// The requesting tool or component
        source: String,
    },
    /// When an elicitation result is received
    ElicitationResult {
        /// The question that was asked
        question: String,
        /// The user's response
        response: String,
    },
    /// When a task is created (Claude Code standard)
    TaskCreated {
        /// The task ID
        task_id: String,
        /// Brief task description
        subject: String,
        /// Task priority
        priority: String,
    },
    /// When a task is completed (Claude Code standard)
    TaskCompleted {
        /// The task ID
        task_id: String,
        /// Brief task description
        subject: String,
    },
}

impl HookEvent {
    /// Get the event type for this hook event
    pub fn event_type(&self) -> HookEventType {
        match self {
            Self::PreToolUse { .. } => HookEventType::PreToolUse,
            Self::PostToolUse { .. } => HookEventType::PostToolUse,
            Self::SessionStart { .. } => HookEventType::SessionStart,
            Self::SessionEnd { .. } => HookEventType::SessionEnd,
            Self::Notification { .. } => HookEventType::Notification,
            Self::UserPromptSubmit { .. } => HookEventType::UserPromptSubmit,
            Self::TeamTaskCreated { .. } => HookEventType::TeamTaskCreated,
            Self::TeamTaskCompleted { .. } => HookEventType::TeamTaskCompleted,
            Self::TeammateIdle { .. } => HookEventType::TeammateIdle,
            Self::PreCompact { .. } => HookEventType::PreCompact,
            Self::SubagentStart { .. } => HookEventType::SubagentStart,
            Self::SubagentStop { .. } => HookEventType::SubagentStop,
            Self::PermissionDenied { .. } => HookEventType::PermissionDenied,
            Self::Stop { .. } => HookEventType::Stop,
            Self::PostToolUseFailure { .. } => HookEventType::PostToolUseFailure,
            Self::PostCompact { .. } => HookEventType::PostCompact,
            Self::StopFailure { .. } => HookEventType::StopFailure,
            Self::FileChanged { .. } => HookEventType::FileChanged,
            Self::CwdChanged { .. } => HookEventType::CwdChanged,
            Self::PermissionRequest { .. } => HookEventType::PermissionRequest,
            Self::UserPromptExpansion { .. } => HookEventType::UserPromptExpansion,
            Self::PostToolBatch { .. } => HookEventType::PostToolBatch,
            Self::ConfigChange { .. } => HookEventType::ConfigChange,
            Self::InstructionsLoaded { .. } => HookEventType::InstructionsLoaded,
            Self::WorktreeCreate { .. } => HookEventType::WorktreeCreate,
            Self::WorktreeRemove { .. } => HookEventType::WorktreeRemove,
            Self::Elicitation { .. } => HookEventType::Elicitation,
            Self::ElicitationResult { .. } => HookEventType::ElicitationResult,
            Self::TaskCreated { .. } => HookEventType::TaskCreated,
            Self::TaskCompleted { .. } => HookEventType::TaskCompleted,
        }
    }

    /// Get the matchable subject for this event.
    /// For tool events, this is the tool name.
    /// For other events, this is the stringified event data.
    pub fn match_subject(&self) -> String {
        match self {
            Self::PreToolUse { tool_name, .. } => tool_name.clone(),
            Self::PostToolUse { tool_name, .. } => tool_name.clone(),
            Self::SessionStart { session_id } => session_id.clone(),
            Self::SessionEnd { session_id } => session_id.clone(),
            Self::Notification { message } => message.clone(),
            Self::UserPromptSubmit { prompt } => prompt.clone(),
            Self::TeamTaskCreated { subject, .. } => subject.clone(),
            Self::TeamTaskCompleted { subject, .. } => subject.clone(),
            Self::TeammateIdle { agent_name, .. } => agent_name.clone(),
            Self::PreCompact { messages_count, .. } => messages_count.to_string(),
            Self::SubagentStart { agent_id, .. } => agent_id.clone(),
            Self::SubagentStop { agent_id, .. } => agent_id.clone(),
            Self::PermissionDenied { tool_name, .. } => tool_name.clone(),
            Self::Stop {
                tool_calls_count, ..
            } => tool_calls_count.to_string(),
            Self::PostToolUseFailure { tool_name, .. } => tool_name.clone(),
            Self::PostCompact { tokens_freed, .. } => tokens_freed.to_string(),
            Self::StopFailure { error } => error.clone(),
            Self::FileChanged { path, .. } => path.clone(),
            Self::CwdChanged { new_cwd, .. } => new_cwd.clone(),
            Self::PermissionRequest { tool_name, .. } => tool_name.clone(),
            Self::UserPromptExpansion {
                expanded_prompt, ..
            } => expanded_prompt.clone(),
            Self::PostToolBatch { tool_names, .. } => tool_names.join(","),
            Self::ConfigChange { config_path, .. } => config_path.clone(),
            Self::InstructionsLoaded { files_count, .. } => files_count.to_string(),
            Self::WorktreeCreate { path, .. } => path.clone(),
            Self::WorktreeRemove { path } => path.clone(),
            Self::Elicitation { source, .. } => source.clone(),
            Self::ElicitationResult { question, .. } => question.clone(),
            Self::TaskCreated { subject, .. } => subject.clone(),
            Self::TaskCompleted { subject, .. } => subject.clone(),
        }
    }

    /// Serialize the event to JSON for passing as stdin to hook commands
    pub fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── HookEventType ────────────────────────────────────────────────────

    #[test]
    fn test_event_type_display_roundtrip() {
        let all_types = [
            HookEventType::PreToolUse,
            HookEventType::PostToolUse,
            HookEventType::SessionStart,
            HookEventType::SessionEnd,
            HookEventType::Notification,
            HookEventType::UserPromptSubmit,
            HookEventType::PreCompact,
            HookEventType::Stop,
            HookEventType::FileChanged,
            HookEventType::CwdChanged,
        ];
        for ty in &all_types {
            let s = ty.to_string();
            assert_eq!(HookEventType::from_str_lossy(&s), Some(ty.clone()));
        }
    }

    #[test]
    fn test_from_str_lossy_unknown() {
        assert_eq!(HookEventType::from_str_lossy("NonExistent"), None);
        assert_eq!(HookEventType::from_str_lossy(""), None);
    }

    #[test]
    fn test_from_str_lossy_all_variants() {
        let all_strs = [
            "PreToolUse",
            "PostToolUse",
            "SessionStart",
            "SessionEnd",
            "Notification",
            "UserPromptSubmit",
            "TeamTaskCreated",
            "TeamTaskCompleted",
            "TeammateIdle",
            "PreCompact",
            "SubagentStart",
            "SubagentStop",
            "PermissionDenied",
            "Stop",
            "PostToolUseFailure",
            "PostCompact",
            "StopFailure",
            "FileChanged",
            "CwdChanged",
            "PermissionRequest",
            "UserPromptExpansion",
            "PostToolBatch",
            "ConfigChange",
            "InstructionsLoaded",
            "WorktreeCreate",
            "WorktreeRemove",
            "Elicitation",
            "ElicitationResult",
            "TaskCreated",
            "TaskCompleted",
        ];
        for s in &all_strs {
            assert!(HookEventType::from_str_lossy(s).is_some(), "Failed for {s}");
        }
    }

    #[test]
    fn test_event_type_serialization_roundtrip() {
        let types = [
            HookEventType::PreToolUse,
            HookEventType::Stop,
            HookEventType::FileChanged,
        ];
        for ty in &types {
            let json = serde_json::to_string(ty).unwrap();
            let parsed: HookEventType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, *ty);
        }
    }

    // ── HookEvent ────────────────────────────────────────────────────────

    #[test]
    fn test_pre_tool_use_event_type() {
        let event = HookEvent::PreToolUse {
            tool_name: "bash".to_string(),
            input: serde_json::json!({"command": "ls"}),
        };
        assert_eq!(event.event_type(), HookEventType::PreToolUse);
        assert_eq!(event.match_subject(), "bash");
    }

    #[test]
    fn test_post_tool_use_event_type() {
        let event = HookEvent::PostToolUse {
            tool_name: "read".to_string(),
            input: serde_json::json!({"path": "/tmp"}),
            output: serde_json::json!("contents"),
        };
        assert_eq!(event.event_type(), HookEventType::PostToolUse);
        assert_eq!(event.match_subject(), "read");
    }

    #[test]
    fn test_session_events() {
        let start = HookEvent::SessionStart {
            session_id: "s1".into(),
        };
        let end = HookEvent::SessionEnd {
            session_id: "s1".into(),
        };
        assert_eq!(start.event_type(), HookEventType::SessionStart);
        assert_eq!(end.event_type(), HookEventType::SessionEnd);
        assert_eq!(start.match_subject(), "s1");
    }

    #[test]
    fn test_notification_event() {
        let event = HookEvent::Notification {
            message: "hello".into(),
        };
        assert_eq!(event.event_type(), HookEventType::Notification);
        assert_eq!(event.match_subject(), "hello");
    }

    #[test]
    fn test_user_prompt_submit() {
        let event = HookEvent::UserPromptSubmit {
            prompt: "fix bug".into(),
        };
        assert_eq!(event.event_type(), HookEventType::UserPromptSubmit);
        assert_eq!(event.match_subject(), "fix bug");
    }

    #[test]
    fn test_pre_compact() {
        let event = HookEvent::PreCompact {
            messages_count: 50,
            estimated_tokens: 80000,
        };
        assert_eq!(event.event_type(), HookEventType::PreCompact);
        assert_eq!(event.match_subject(), "50");
    }

    #[test]
    fn test_post_compact() {
        let event = HookEvent::PostCompact {
            messages_before: 50,
            messages_after: 10,
            tokens_freed: 30000,
        };
        assert_eq!(event.event_type(), HookEventType::PostCompact);
        assert_eq!(event.match_subject(), "30000");
    }

    #[test]
    fn test_stop_event() {
        let event = HookEvent::Stop {
            tool_calls_count: 5,
            should_continue: false,
        };
        assert_eq!(event.event_type(), HookEventType::Stop);
        assert_eq!(event.match_subject(), "5");
    }

    #[test]
    fn test_file_changed_event() {
        let event = HookEvent::FileChanged {
            path: "/src/main.rs".into(),
            change_type: "modify".into(),
        };
        assert_eq!(event.event_type(), HookEventType::FileChanged);
        assert_eq!(event.match_subject(), "/src/main.rs");
    }

    #[test]
    fn test_cwd_changed_event() {
        let event = HookEvent::CwdChanged {
            old_cwd: "/old".into(),
            new_cwd: "/new".into(),
        };
        assert_eq!(event.event_type(), HookEventType::CwdChanged);
        assert_eq!(event.match_subject(), "/new");
    }

    #[test]
    fn test_permission_denied() {
        let event = HookEvent::PermissionDenied {
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
            retry_count: 3,
        };
        assert_eq!(event.event_type(), HookEventType::PermissionDenied);
        assert_eq!(event.match_subject(), "bash");
    }

    #[test]
    fn test_post_tool_batch() {
        let event = HookEvent::PostToolBatch {
            tool_names: vec!["read".into(), "edit".into(), "bash".into()],
            success_count: 2,
            failure_count: 1,
        };
        assert_eq!(event.event_type(), HookEventType::PostToolBatch);
        assert_eq!(event.match_subject(), "read,edit,bash");
    }

    #[test]
    fn test_team_events() {
        let created = HookEvent::TeamTaskCreated {
            task_id: "t1".into(),
            team_name: "team".into(),
            agent_name: Some("agent".into()),
            subject: "task".into(),
            priority: "high".into(),
        };
        let completed = HookEvent::TeamTaskCompleted {
            task_id: "t1".into(),
            team_name: "team".into(),
            agent_name: "agent".into(),
            subject: "task".into(),
        };
        let idle = HookEvent::TeammateIdle {
            team_name: "team".into(),
            agent_name: "agent".into(),
            available_tasks: 3,
        };
        assert_eq!(created.event_type(), HookEventType::TeamTaskCreated);
        assert_eq!(completed.event_type(), HookEventType::TeamTaskCompleted);
        assert_eq!(idle.event_type(), HookEventType::TeammateIdle);
    }

    // ── Serialization ────────────────────────────────────────────────────

    #[test]
    fn test_hook_event_json_roundtrip() {
        let event = HookEvent::PreToolUse {
            tool_name: "bash".into(),
            input: serde_json::json!({"command": "ls -la"}),
        };
        let bytes = event.to_json_bytes();
        assert!(!bytes.is_empty());
        let parsed: HookEvent = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.event_type(), HookEventType::PreToolUse);
    }

    #[test]
    fn test_hook_event_json_has_pascal_case() {
        let event = HookEvent::PostToolUse {
            tool_name: "x".into(),
            input: serde_json::json!(null),
            output: serde_json::json!(null),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(
            json.contains("\"PostToolUse\""),
            "Expected PascalCase, got: {json}"
        );
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<HookEventType>();
        assert_send_sync::<HookEvent>();
    }
}
