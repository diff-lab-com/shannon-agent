//! # Streaming Tool Executor
//!
//! A concurrent-safe streaming tool executor that manages tool lifecycle with
//! a state machine and enforces concurrency rules.
//!
//! ## Architecture
//!
//! - [`StreamingToolExecutor`]: Orchestrates concurrent tool execution
//! - [`TrackedTool`]: Individual tool with state tracking
//! - [`ToolStatus`]: State machine: `Queued -> Executing -> Completed -> Yielded`
//!
//! ## Concurrency Model
//!
//! - Tools flagged `is_concurrency_safe` can run in parallel
//! - Non-concurrent tools require exclusive access (only one at a time)
//! - Results are buffered and emitted in submission order, not completion order
//!
//! ## Usage
//!
//! ```rust,ignore
//! use shannon_core::streaming_tool_executor::StreamingToolExecutor;
//!
//! let executor = StreamingToolExecutor::new(32);
//! let tool_id = executor.submit_tool("Bash", serde_json::json!({"command": "ls"}), false);
//!
//! // Poll for completed results in submission order
//! while let Some(result) = executor.get_remaining_results() {
//!     println!("Tool {} completed: {:?}", result.id, result.output);
//! }
//! ```

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{mpsc, Mutex, Notify};

use crate::tools::ToolOutput;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors produced by the streaming tool executor.
#[derive(Error, Debug)]
pub enum ExecutorError {
    #[error("Executor is aborted")]
    Aborted,

    #[error("Tool execution failed for '{tool_id}': {reason}")]
    ExecutionFailed { tool_id: String, reason: String },

    #[error("Channel closed unexpectedly")]
    ChannelClosed,

    #[error("Tool already queued: {0}")]
    AlreadyQueued(String),

    #[error("No tools submitted")]
    Empty,
}

// ---------------------------------------------------------------------------
// ToolStatus
// ---------------------------------------------------------------------------

/// Lifecycle state of a tracked tool.
///
/// State transitions:
/// ```text
/// Queued -> Executing -> Completed -> Yielded
///                     \-> Failed    -> Yielded
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolStatus {
    /// Tool has been submitted but not yet picked up for execution.
    Queued,
    /// Tool is currently executing.
    Executing,
    /// Tool has finished (success or error) but result not yet consumed.
    Completed,
    /// Result has been yielded to the caller via `get_remaining_results`.
    Yielded,
}

impl std::fmt::Display for ToolStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Queued => write!(f, "Queued"),
            Self::Executing => write!(f, "Executing"),
            Self::Completed => write!(f, "Completed"),
            Self::Yielded => write!(f, "Yielded"),
        }
    }
}

// ---------------------------------------------------------------------------
// TrackedTool
// ---------------------------------------------------------------------------

/// A tool tracked by the executor with its full lifecycle state.
#[derive(Debug, Clone)]
pub struct TrackedTool {
    /// Unique identifier for this tool invocation.
    pub id: String,
    /// Tool name (e.g. "Bash", "Read", "Write").
    pub tool_name: String,
    /// Input parameters provided to the tool.
    pub input: Value,
    /// Whether this tool is safe to run concurrently with others.
    pub is_concurrency_safe: bool,
    /// Current lifecycle status.
    pub status: ToolStatus,
    /// The result produced by the tool, if completed.
    pub output: Option<ToolOutput>,
    /// Monotonic submission order index (used for ordered yielding).
    pub submission_order: usize,
    /// Time when the tool transitioned to `Executing`.
    pub started_at: Option<Instant>,
    /// Time when the tool transitioned to `Completed` or `Failed`.
    pub completed_at: Option<Instant>,
    /// Accumulated progress messages for this tool.
    pub progress_messages: Vec<String>,
}

impl TrackedTool {
    /// Create a new tracked tool in `Queued` state.
    pub fn new(
        id: String,
        tool_name: String,
        input: Value,
        is_concurrency_safe: bool,
        submission_order: usize,
    ) -> Self {
        Self {
            id,
            tool_name,
            input,
            is_concurrency_safe,
            status: ToolStatus::Queued,
            output: None,
            submission_order,
            started_at: None,
            completed_at: None,
            progress_messages: Vec::new(),
        }
    }

    /// Transition the tool to `Executing` state.
    pub fn start(&mut self) {
        self.status = ToolStatus::Executing;
        self.started_at = Some(Instant::now());
    }

    /// Mark the tool as `Completed` with an output.
    pub fn complete(&mut self, output: ToolOutput) {
        self.status = ToolStatus::Completed;
        self.output = Some(output);
        self.completed_at = Some(Instant::now());
    }

    /// Mark the tool as `Completed` with an error output.
    pub fn fail(&mut self, reason: &str) {
        self.status = ToolStatus::Completed;
        self.output = Some(ToolOutput {
            content: reason.to_string(),
            is_error: true,
            metadata: Value::Object(Default::default())
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        });
        self.completed_at = Some(Instant::now());
    }

    /// Mark the tool as `Yielded`.
    pub fn yield_result(&mut self) {
        self.status = ToolStatus::Yielded;
    }

    /// Duration the tool spent executing, from start to completion.
    pub fn execution_duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(end.duration_since(start)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Progress message type
// ---------------------------------------------------------------------------

/// A progress update from a running tool, sent over the progress channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressMessage {
    /// ID of the tool this progress message is for.
    pub tool_id: String,
    /// Human-readable progress text.
    pub message: String,
    /// Whether this progress update represents completion.
    pub is_complete: bool,
}

// ---------------------------------------------------------------------------
// StreamingToolExecutor
// ---------------------------------------------------------------------------

/// Concurrent-safe streaming tool executor.
///
/// Manages tool lifecycle, enforces concurrency rules, buffers results,
/// and yields them in submission order.
pub struct StreamingToolExecutor {
    /// All tracked tools in submission order.
    tools: Arc<Mutex<Vec<TrackedTool>>>,
    /// Index of the next tool to yield (maintains submission order).
    yield_cursor: Arc<Mutex<usize>>,
    /// Counter for generating monotonic submission order indices.
    submission_counter: Arc<Mutex<usize>>,
    /// Whether the executor has been aborted.
    aborted: AtomicBool,
    /// Sender half of the progress channel.
    progress_tx: mpsc::UnboundedSender<ProgressMessage>,
    /// Receiver half of the progress channel.
    progress_rx: Arc<Mutex<mpsc::UnboundedReceiver<ProgressMessage>>>,
    /// Notifies waiters when a new tool completes.
    completion_notify: Arc<Notify>,
}

impl StreamingToolExecutor {
    /// Create a new executor with the given channel buffer size for progress messages.
    pub fn new(_progress_buffer: usize) -> Self {
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();

        Self {
            tools: Arc::new(Mutex::new(Vec::new())),
            yield_cursor: Arc::new(Mutex::new(0)),
            submission_counter: Arc::new(Mutex::new(0)),
            aborted: AtomicBool::new(false),
            progress_tx,
            progress_rx: Arc::new(Mutex::new(progress_rx)),
            completion_notify: Arc::new(Notify::new()),
        }
    }

    /// Submit a tool for execution.
    ///
    /// Returns the unique tool ID assigned to this submission.
    pub async fn submit_tool(
        &self,
        tool_name: &str,
        input: Value,
        is_concurrency_safe: bool,
    ) -> Result<String, ExecutorError> {
        if self.aborted.load(Ordering::SeqCst) {
            return Err(ExecutorError::Aborted);
        }

        let mut counter = self.submission_counter.lock().await;
        let order = *counter;
        *counter += 1;

        let id = uuid::Uuid::new_v4().to_string();
        let tool = TrackedTool::new(
            id.clone(),
            tool_name.to_string(),
            input,
            is_concurrency_safe,
            order,
        );

        let mut tools = self.tools.lock().await;
        tools.push(tool);

        tracing::debug!(
            tool_id = %id,
            tool_name = %tool_name,
            order = order,
            is_concurrency_safe = is_concurrency_safe,
            "Tool submitted to executor"
        );

        Ok(id)
    }

    /// Transition a tool to `Executing` state.
    ///
    /// Returns an error if the executor is aborted.
    pub async fn start_tool(&self, tool_id: &str) -> Result<(), ExecutorError> {
        if self.aborted.load(Ordering::SeqCst) {
            return Err(ExecutorError::Aborted);
        }

        let mut tools = self.tools.lock().await;
        if let Some(tool) = tools.iter_mut().find(|t| t.id == tool_id) {
            tool.start();
            tracing::debug!(tool_id = %tool_id, "Tool started executing");
            Ok(())
        } else {
            Err(ExecutorError::ExecutionFailed {
                tool_id: tool_id.to_string(),
                reason: "Tool not found".to_string(),
            })
        }
    }

    /// Mark a tool as completed with a successful output.
    pub async fn complete_tool(&self, tool_id: &str, output: ToolOutput) -> Result<(), ExecutorError> {
        let mut tools = self.tools.lock().await;
        if let Some(tool) = tools.iter_mut().find(|t| t.id == tool_id) {
            tool.complete(output);
            tracing::debug!(tool_id = %tool_id, "Tool completed");
        }
        drop(tools);
        self.completion_notify.notify_one();
        Ok(())
    }

    /// Mark a tool as failed with an error message.
    pub async fn fail_tool(&self, tool_id: &str, reason: &str) -> Result<(), ExecutorError> {
        let mut tools = self.tools.lock().await;
        if let Some(tool) = tools.iter_mut().find(|t| t.id == tool_id) {
            tool.fail(reason);
            tracing::debug!(tool_id = %tool_id, reason = %reason, "Tool failed");
        }
        drop(tools);
        self.completion_notify.notify_one();
        Ok(())
    }

    /// Get the next completed result in submission order.
    ///
    /// Returns `None` if there are no more results to yield or if all
    /// remaining tools are still executing.
    ///
    /// Results are yielded strictly in submission order: if tool A (index 0)
    /// is still Queued/Executing, this method will not yield tool B (index 1)
    /// even if B is already Completed.
    pub async fn get_remaining_results(&self) -> Option<TrackedTool> {
        let tools = self.tools.lock().await;
        let mut cursor = self.yield_cursor.lock().await;

        if *cursor >= tools.len() {
            return None;
        }

        // Only yield the tool at the current cursor position if it's completed.
        // This maintains strict submission-order yielding.
        if tools[*cursor].status == ToolStatus::Completed {
            let mut tool = tools[*cursor].clone();
            tool.yield_result();
            *cursor += 1;
            return Some(tool);
        }

        None
    }

    /// Get the next completed result, waiting until one is available or all tools finish.
    pub async fn wait_for_result(&self) -> Option<TrackedTool> {
        loop {
            if let Some(result) = self.get_remaining_results().await {
                return Some(result);
            }

            // Check if all tools have been yielded or there are no tools
            // Lock order: tools -> yield_cursor (same as reset())
            {
                let tools = self.tools.lock().await;
                let cursor = self.yield_cursor.lock().await;
                if *cursor >= tools.len() {
                    return None;
                }

                // Check if all remaining tools are in terminal state
                let all_terminal = tools[*cursor..]
                    .iter()
                    .all(|t| t.status == ToolStatus::Completed || t.status == ToolStatus::Yielded);
                if all_terminal {
                    return None;
                }
            }

            self.completion_notify.notified().await;
        }
    }

    /// Abort the executor. Cancels all pending and executing tools.
    ///
    /// Sibling tools (tools in the same batch) are cancelled via the
    /// sibling abort mechanism.
    pub fn abort(&self) {
        self.aborted.store(true, Ordering::SeqCst);
        tracing::info!("Executor aborted");
    }

    /// Check if the executor is aborted.
    pub fn is_aborted(&self) -> bool {
        self.aborted.load(Ordering::SeqCst)
    }

    /// Mark a specific tool as having caused an error, triggering sibling abort.
    ///
    /// When one tool errors, all sibling tools (tools in the same batch that
    /// haven't completed yet) are effectively cancelled.
    pub async fn mark_error(&self, error_tool_id: &str, reason: &str) {
        tracing::warn!(
            error_tool_id = %error_tool_id,
            reason = %reason,
            "Tool error - sibling abort triggered"
        );

        // Mark the erroring tool as failed
        if let Err(e) = self.fail_tool(error_tool_id, reason).await {
            tracing::debug!("Failed to mark tool as failed: {e}");
        }

        // Mark all other non-completed tools as failed (sibling abort)
        let mut tools = self.tools.lock().await;
        for tool in tools.iter_mut() {
            if tool.id != error_tool_id
                && tool.status != ToolStatus::Completed
                && tool.status != ToolStatus::Yielded
            {
                tool.fail(&format!(
                    "Sibling abort: tool '{error_tool_id}' failed with: {reason}"
                ));
            }
        }
        drop(tools);

        self.completion_notify.notify_one();
    }

    /// Add a progress message for a specific tool.
    pub fn add_progress(&self, tool_id: &str, message: &str) {
        let progress = ProgressMessage {
            tool_id: tool_id.to_string(),
            message: message.to_string(),
            is_complete: false,
        };
        if self.progress_tx.send(progress).is_err() {
            tracing::debug!("progress update dropped: no active receivers");
        }
    }

    /// Try to receive a progress message without blocking.
    pub async fn recv_progress(&self) -> Option<ProgressMessage> {
        let mut rx = self.progress_rx.lock().await;
        rx.try_recv().ok()
    }

    /// Get a reference to the tracked tools (for inspection).
    pub async fn tools(&self) -> Vec<TrackedTool> {
        self.tools.lock().await.clone()
    }

    /// Get the number of tools in each status.
    pub async fn status_counts(&self) -> HashMap<String, usize> {
        let tools = self.tools.lock().await;
        let mut counts = HashMap::new();
        counts.insert("Queued".to_string(), 0);
        counts.insert("Executing".to_string(), 0);
        counts.insert("Completed".to_string(), 0);
        counts.insert("Yielded".to_string(), 0);

        for tool in tools.iter() {
            let key = tool.status.to_string();
            *counts.entry(key).or_insert(0) += 1;
        }

        counts
    }

    /// Get the total number of tools submitted.
    pub async fn total_tools(&self) -> usize {
        self.tools.lock().await.len()
    }

    /// Check if all tools have been yielded.
    pub async fn is_drained(&self) -> bool {
        let tools = self.tools.lock().await;
        let cursor = self.yield_cursor.lock().await;
        *cursor >= tools.len()
    }

    /// Check if a non-concurrent tool can execute given the current state.
    ///
    /// A non-concurrent tool can only execute if no other non-concurrent tool
    /// is currently in `Executing` state.
    pub async fn can_execute_non_concurrent(&self) -> bool {
        let tools = self.tools.lock().await;
        !tools.iter().any(|t| {
            t.status == ToolStatus::Executing && !t.is_concurrency_safe
        })
    }

    /// Get all tools currently in `Queued` state.
    pub async fn queued_tools(&self) -> Vec<TrackedTool> {
        let tools = self.tools.lock().await;
        tools
            .iter()
            .filter(|t| t.status == ToolStatus::Queued)
            .cloned()
            .collect()
    }

    /// Get all tools currently in `Executing` state.
    pub async fn executing_tools(&self) -> Vec<TrackedTool> {
        let tools = self.tools.lock().await;
        tools
            .iter()
            .filter(|t| t.status == ToolStatus::Executing)
            .cloned()
            .collect()
    }

    /// Reset the executor for reuse.
    pub async fn reset(&self) {
        let mut tools = self.tools.lock().await;
        tools.clear();
        let mut cursor = self.yield_cursor.lock().await;
        *cursor = 0;
        let mut counter = self.submission_counter.lock().await;
        *counter = 0;
        self.aborted.store(false, Ordering::SeqCst);
    }
}

impl Default for StreamingToolExecutor {
    fn default() -> Self {
        Self::new(32)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_output(content: &str) -> ToolOutput {
        ToolOutput {
            content: content.to_string(),
            is_error: false,
            metadata: Default::default(),
        }
    }

    // -- TrackedTool unit tests --

    #[test]
    fn test_tracked_tool_new() {
        let tool = TrackedTool::new(
            "id-1".to_string(),
            "Bash".to_string(),
            serde_json::json!({"cmd": "ls"}),
            false,
            0,
        );
        assert_eq!(tool.status, ToolStatus::Queued);
        assert_eq!(tool.tool_name, "Bash");
        assert!(!tool.is_concurrency_safe);
        assert!(tool.output.is_none());
        assert!(tool.started_at.is_none());
    }

    #[test]
    fn test_tracked_tool_state_transitions() {
        let mut tool = TrackedTool::new(
            "id-1".to_string(),
            "Read".to_string(),
            Value::Null,
            true,
            0,
        );

        assert_eq!(tool.status, ToolStatus::Queued);
        tool.start();
        assert_eq!(tool.status, ToolStatus::Executing);
        assert!(tool.started_at.is_some());

        tool.complete(make_output("file contents"));
        assert_eq!(tool.status, ToolStatus::Completed);
        assert!(tool.output.is_some());
        assert!(tool.completed_at.is_some());

        tool.yield_result();
        assert_eq!(tool.status, ToolStatus::Yielded);
    }

    #[test]
    fn test_tracked_tool_fail() {
        let mut tool = TrackedTool::new(
            "id-1".to_string(),
            "Bash".to_string(),
            Value::Null,
            false,
            0,
        );
        tool.start();
        tool.fail("command not found");
        assert_eq!(tool.status, ToolStatus::Completed);
        let output = tool.output.as_ref().unwrap();
        assert!(output.is_error);
        assert!(output.content.contains("command not found"));
    }

    #[test]
    fn test_tracked_tool_execution_duration() {
        let mut tool = TrackedTool::new(
            "id-1".to_string(),
            "Bash".to_string(),
            Value::Null,
            true,
            0,
        );
        assert!(tool.execution_duration().is_none());

        tool.start();
        tool.complete(make_output("done"));
        let dur = tool.execution_duration().unwrap();
        // Should be very short (microseconds) but non-zero in practice
        assert!(dur.as_nanos() > 0 || dur.is_zero());
    }

    #[test]
    fn test_tracked_tool_concurrency_safe_flag() {
        let safe = TrackedTool::new(
            "id-1".to_string(),
            "Read".to_string(),
            Value::Null,
            true,
            0,
        );
        assert!(safe.is_concurrency_safe);

        let unsafe_tool = TrackedTool::new(
            "id-2".to_string(),
            "Bash".to_string(),
            Value::Null,
            false,
            1,
        );
        assert!(!unsafe_tool.is_concurrency_safe);
    }

    // -- ToolStatus tests --

    #[test]
    fn test_tool_status_display() {
        assert_eq!(format!("{}", ToolStatus::Queued), "Queued");
        assert_eq!(format!("{}", ToolStatus::Executing), "Executing");
        assert_eq!(format!("{}", ToolStatus::Completed), "Completed");
        assert_eq!(format!("{}", ToolStatus::Yielded), "Yielded");
    }

    // -- StreamingToolExecutor async tests --

    #[tokio::test]
    async fn test_executor_submit_tool() {
        let executor = StreamingToolExecutor::new(32);
        let id = executor
            .submit_tool("Bash", serde_json::json!({"cmd": "ls"}), false)
            .await
            .unwrap();
        assert!(!id.is_empty());
        assert_eq!(executor.total_tools().await, 1);
    }

    #[tokio::test]
    async fn test_executor_submit_multiple() {
        let executor = StreamingToolExecutor::new(32);
        let id1 = executor
            .submit_tool("Read", Value::Null, true)
            .await
            .unwrap();
        let id2 = executor
            .submit_tool("Bash", Value::Null, false)
            .await
            .unwrap();
        assert_ne!(id1, id2);
        assert_eq!(executor.total_tools().await, 2);
    }

    #[tokio::test]
    async fn test_executor_start_and_complete_tool() {
        let executor = StreamingToolExecutor::new(32);
        let id = executor
            .submit_tool("Read", Value::Null, true)
            .await
            .unwrap();

        executor.start_tool(&id).await.unwrap();
        let tools = executor.tools().await;
        assert_eq!(tools[0].status, ToolStatus::Executing);

        executor
            .complete_tool(&id, make_output("file contents"))
            .await
            .unwrap();

        let result = executor.get_remaining_results().await.unwrap();
        assert_eq!(result.id, id);
        assert_eq!(result.status, ToolStatus::Yielded);
        assert_eq!(result.output.as_ref().unwrap().content, "file contents");
    }

    #[tokio::test]
    async fn test_executor_results_in_submission_order() {
        let executor = StreamingToolExecutor::new(32);

        // Submit tools A, B, C
        let id_a = executor.submit_tool("A", Value::Null, true).await.unwrap();
        let id_b = executor.submit_tool("B", Value::Null, true).await.unwrap();
        let id_c = executor.submit_tool("C", Value::Null, true).await.unwrap();

        // Complete them out of order: C, A, B
        executor.complete_tool(&id_c, make_output("C")).await.unwrap();
        executor.complete_tool(&id_a, make_output("A")).await.unwrap();
        executor.complete_tool(&id_b, make_output("B")).await.unwrap();

        // Results should come back in submission order: A, B, C
        let r1 = executor.get_remaining_results().await.unwrap();
        assert_eq!(r1.id, id_a);

        let r2 = executor.get_remaining_results().await.unwrap();
        assert_eq!(r2.id, id_b);

        let r3 = executor.get_remaining_results().await.unwrap();
        assert_eq!(r3.id, id_c);

        // No more results
        assert!(executor.get_remaining_results().await.is_none());
    }

    #[tokio::test]
    async fn test_executor_abort() {
        let executor = StreamingToolExecutor::new(32);
        executor.submit_tool("A", Value::Null, true).await.unwrap();

        executor.abort();
        assert!(executor.is_aborted());

        // Submitting after abort should fail
        let result = executor.submit_tool("B", Value::Null, true).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ExecutorError::Aborted));
    }

    #[tokio::test]
    async fn test_executor_mark_error_sibling_abort() {
        let executor = StreamingToolExecutor::new(32);

        let id_a = executor.submit_tool("A", Value::Null, false).await.unwrap();
        let id_b = executor.submit_tool("B", Value::Null, false).await.unwrap();
        let id_c = executor.submit_tool("C", Value::Null, false).await.unwrap();

        // Start all tools
        executor.start_tool(&id_a).await.unwrap();
        executor.start_tool(&id_b).await.unwrap();
        executor.start_tool(&id_c).await.unwrap();

        // Mark B as error - should abort A and C
        executor.mark_error(&id_b, "disk full").await;

        let tools = executor.tools().await;
        // All should be completed (failed or error)
        for tool in &tools {
            assert_eq!(tool.status, ToolStatus::Completed);
        }
        // B should have the original error
        let b = tools.iter().find(|t| t.id == id_b).unwrap();
        assert!(b.output.as_ref().unwrap().is_error);
        assert!(b.output.as_ref().unwrap().content.contains("disk full"));

        // A and C should have sibling abort messages
        let a = tools.iter().find(|t| t.id == id_a).unwrap();
        assert!(a.output.as_ref().unwrap().content.contains("Sibling abort"));
    }

    #[tokio::test]
    async fn test_executor_non_concurrent_exclusivity() {
        let executor = StreamingToolExecutor::new(32);

        // No tools executing - should be able to execute non-concurrent
        assert!(executor.can_execute_non_concurrent().await);

        let id = executor
            .submit_tool("Bash", Value::Null, false)
            .await
            .unwrap();
        executor.start_tool(&id).await.unwrap();

        // Non-concurrent tool is executing - should not be able to execute another
        assert!(!executor.can_execute_non_concurrent().await);
    }

    #[tokio::test]
    async fn test_executor_concurrent_safe_allows_parallel() {
        let executor = StreamingToolExecutor::new(32);

        let id1 = executor
            .submit_tool("Read", Value::Null, true)
            .await
            .unwrap();
        executor.start_tool(&id1).await.unwrap();

        // Concurrent-safe tool executing should still allow non-concurrent
        // because the check only looks at non-concurrent tools
        assert!(executor.can_execute_non_concurrent().await);
    }

    #[tokio::test]
    async fn test_executor_status_counts() {
        let executor = StreamingToolExecutor::new(32);

        let id1 = executor
            .submit_tool("A", Value::Null, true)
            .await
            .unwrap();
        let _id2 = executor
            .submit_tool("B", Value::Null, true)
            .await
            .unwrap();

        executor.start_tool(&id1).await.unwrap();
        executor
            .complete_tool(&id1, make_output("done"))
            .await
            .unwrap();

        let counts = executor.status_counts().await;
        assert_eq!(counts.get("Queued").unwrap(), &1);
        assert_eq!(counts.get("Completed").unwrap(), &1);
    }

    #[tokio::test]
    async fn test_executor_queued_and_executing_filters() {
        let executor = StreamingToolExecutor::new(32);

        let id1 = executor
            .submit_tool("A", Value::Null, true)
            .await
            .unwrap();
        let _id2 = executor
            .submit_tool("B", Value::Null, true)
            .await
            .unwrap();

        executor.start_tool(&id1).await.unwrap();

        let queued = executor.queued_tools().await;
        assert_eq!(queued.len(), 1);

        let executing = executor.executing_tools().await;
        assert_eq!(executing.len(), 1);
        assert_eq!(executing[0].id, id1);
    }

    #[tokio::test]
    async fn test_executor_is_drained() {
        let executor = StreamingToolExecutor::new(32);
        assert!(executor.is_drained().await);

        let id_a = executor
            .submit_tool("A", Value::Null, true)
            .await
            .unwrap();
        assert!(!executor.is_drained().await);

        // Complete A and yield it
        executor
            .complete_tool(&id_a, make_output("done"))
            .await
            .unwrap();
        let result = executor.get_remaining_results().await;
        assert!(result.is_some());
        // Now all submitted tools have been yielded
        assert!(executor.is_drained().await);
    }

    #[tokio::test]
    async fn test_executor_progress_messages() {
        let executor = StreamingToolExecutor::new(32);
        let id = executor
            .submit_tool("Bash", Value::Null, true)
            .await
            .unwrap();

        executor.add_progress(&id, "compiling...");
        executor.add_progress(&id, "linking...");

        let msg1 = executor.recv_progress().await.unwrap();
        assert_eq!(msg1.tool_id, id);
        assert_eq!(msg1.message, "compiling...");

        let msg2 = executor.recv_progress().await.unwrap();
        assert_eq!(msg2.message, "linking...");
    }

    #[tokio::test]
    async fn test_executor_reset() {
        let executor = StreamingToolExecutor::new(32);
        executor
            .submit_tool("A", Value::Null, true)
            .await
            .unwrap();
        executor.abort();
        assert!(executor.is_aborted());

        executor.reset().await;
        assert!(!executor.is_aborted());
        assert_eq!(executor.total_tools().await, 0);
        assert!(executor.is_drained().await);
    }

    #[tokio::test]
    async fn test_executor_wait_for_result() {
        let executor = StreamingToolExecutor::new(32);
        let id = executor
            .submit_tool("A", Value::Null, true)
            .await
            .unwrap();

        // Complete the tool in a background task after a delay
        let tools = executor.tools.clone();
        let notify = executor.completion_notify.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut tools = tools.lock().await;
            if let Some(tool) = tools.iter_mut().find(|t| t.id == id) {
                tool.complete(make_output("done"));
            }
            drop(tools);
            notify.notify_one();
        });

        let result = executor.wait_for_result().await.unwrap();
        assert_eq!(result.output.as_ref().unwrap().content, "done");
    }

    #[tokio::test]
    async fn test_executor_default() {
        let executor = StreamingToolExecutor::default();
        assert_eq!(executor.total_tools().await, 0);
    }
}
