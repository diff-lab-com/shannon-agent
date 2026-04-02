//! Shannon Tools - Claude Code tool implementations
//!
//! This crate provides implementations of Claude Code tools including:
//! - File operations (Read, Write, Edit, Glob)
//! - System operations (Bash commands, Sleep)
//! - Web operations (WebFetch, WebSearch)
//! - Agent operations (Agent spawning, messaging)
//! - Task operations (Todo management, task lists)
//! - Notebook operations (NotebookEdit for Jupyter notebooks)
//! - Worktree operations (EnterWorktree, ExitWorktree for git worktrees)
//! - MCP operations (ReadMcpResource, ListMcpResources for MCP servers)
//! - Skill operations (Skill for user-callable skills)
//! - Cron operations (CronCreate, CronDelete, CronList for scheduling)
//! - Messaging operations (SendMessage for team communication)

pub mod file;
pub mod system;
pub mod web;
pub mod agent;
pub mod task;
pub mod notebook;
pub mod worktree;
pub mod mcp;
pub mod messaging;
pub mod todo;
pub mod skill;
pub mod cron;

// Re-exports for convenience
pub use file::{ReadTool, WriteTool, EditTool, GlobTool, FileOperation};
pub use system::{SystemTool, ShellCommand, SleepTool, BashTool, PowerShellTool};
pub use web::{WebFetchTool, WebSearchTool, WebOperation};
pub use agent::{AgentTool, AgentOperation};
pub use task::{TaskTool, TaskOperation};
pub use notebook::{NotebookEditTool, NotebookEditInput, NotebookEditOutput};
pub use worktree::{WorktreeTool, EnterWorktreeInput, EnterWorktreeOutput, ExitWorktreeInput, ExitWorktreeOutput};
pub use mcp::{McpResourceTool, ReadMcpResourceInput, ReadMcpResourceOutput, ListMcpResourcesInput, ListMcpResourcesOutput};
pub use messaging::{SendMessageTool, SendMessageInput, SendMessageOutput};
pub use todo::{TodoWriteTool, TodoWriteInput, TodoWriteOutput};
pub use skill::{SkillTool, SkillInvokeInput, SkillInvokeOutput};
pub use cron::{CronTool, CronCreateInput, CronCreateOutput, CronDeleteInput, CronDeleteOutput, CronListInput, CronListOutput};

// Re-export from shannon_core
pub use shannon_core::{
    tools::{Tool, ToolError, ToolResult, ToolOutput},
};
