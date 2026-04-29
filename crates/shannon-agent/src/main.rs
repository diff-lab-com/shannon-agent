//! Standalone agent binary for out-of-process agent execution.
//!
//! This binary reads JSON-RPC from stdin, executes tasks using the LLM,
//! and writes JSON-RPC notifications/responses to stdout.
//!
//! Usage:
//!   shannon-agent --name worker-1 --model claude-sonnet-4-20250514

use clap::Parser;
use shannon_agents::{
    frame_message, parse_message,
    AgentReadyParams, ExecuteTaskParams, TaskCompleteParams, TaskProgressParams,
    AgentIdleParams,
    JsonRpcMessage, JsonRpcError,
};
use std::io;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod methods {
    pub const EXECUTE_TASK: &str = "execute_task";
    pub const SHUTDOWN: &str = "shutdown";
    pub const PING: &str = "ping";
    pub const AGENT_READY: &str = "agent_ready";
    pub const TASK_PROGRESS: &str = "task_progress";
    pub const TASK_COMPLETE: &str = "task_complete";
    pub const AGENT_IDLE: &str = "agent_idle";
}

#[derive(Parser, Debug, Clone)]
#[command(name = "shannon-agent", about = "Standalone agent process")]
struct Args {
    /// Agent name (must be unique within the team)
    #[arg(long)]
    name: String,

    /// LLM model to use
    #[arg(long)]
    model: Option<String>,

    /// System prompt for the agent
    #[arg(long)]
    system_prompt: Option<String>,

    /// Working directory for the agent
    #[arg(long)]
    workdir: Option<String>,
}

/// Write a JSON-RPC message to stdout (line-delimited).
async fn send_message(msg: &JsonRpcMessage) -> io::Result<()> {
    let line = frame_message(msg)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let mut stdout = tokio::io::stdout();
    stdout.write_all(line.as_bytes()).await?;
    stdout.flush().await?;
    Ok(())
}

/// Send a notification to the coordinator.
async fn notify(method: &str, params: serde_json::Value) {
    let msg = JsonRpcMessage::notification(method, params);
    if let Err(e) = send_message(&msg).await {
        tracing::error!(method = %method, error = %e, "Failed to send notification");
    }
}

/// Send an RPC response (success).
async fn respond(id: i64, result: serde_json::Value) {
    let msg = JsonRpcMessage::response(
        shannon_agents::JsonRpcId::Number(id),
        result,
    );
    if let Err(e) = send_message(&msg).await {
        tracing::error!(id = %id, error = %e, "Failed to send response");
    }
}

/// Send an RPC error response.
async fn respond_error(id: i64, error: JsonRpcError) {
    let msg = JsonRpcMessage::error_response(
        shannon_agents::JsonRpcId::Number(id),
        error,
    );
    if let Err(e) = send_message(&msg).await {
        tracing::error!(id = %id, error = %e, "Failed to send error response");
    }
}

/// Execute a task. In a real implementation this would call the LLM.
/// For now, this is a stub that simulates task execution.
async fn execute_task(params: ExecuteTaskParams, args: &Args) {
    tracing::info!(
        task_id = %params.task_id,
        subject = %params.subject,
        "Executing task"
    );

    // Send a progress notification
    let progress_params = serde_json::to_value(TaskProgressParams {
        task_id: params.task_id.clone(),
        chunk: format!("Starting task: {}", params.subject),
    }).unwrap();
    notify(methods::TASK_PROGRESS, progress_params).await;

    // TODO: In a full implementation, this would:
    // 1. Construct an LLM prompt from the task description
    // 2. Call the LLM client with tool access
    // 3. Stream progress updates
    // 4. Return the final result
    //
    // For now, we acknowledge the task and report completion.
    let output = format!(
        "Task '{}' ({}): execution stub — no LLM client wired yet. Agent: {}",
        params.subject, params.task_id, args.name
    );

    let complete_params = serde_json::to_value(TaskCompleteParams {
        task_id: params.task_id.clone(),
        success: true,
        output,
    }).unwrap();
    notify(methods::TASK_COMPLETE, complete_params).await;

    // Report idle
    let idle_params = serde_json::to_value(AgentIdleParams {
        agent_name: args.name.clone(),
        available_tasks_count: 0,
    }).unwrap();
    notify(methods::AGENT_IDLE, idle_params).await;
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Initialize logging to stderr (stdout is reserved for JSON-RPC)
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("shannon_agent=info".parse().unwrap())
        )
        .init();

    tracing::info!(name = %args.name, "Agent process starting");

    // Send agent_ready notification
    let ready_params = serde_json::to_value(AgentReadyParams {
        agent_name: args.name.clone(),
        capabilities: vec!["general".to_string()],
    }).unwrap();
    notify(methods::AGENT_READY, ready_params).await;

    // Read JSON-RPC from stdin
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    tracing::info!("Listening for JSON-RPC on stdin");

    while let Ok(Some(line)) = lines.next_line().await {
        let msg = match parse_message(&line) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse JSON-RPC message");
                continue;
            }
        };

        match msg.method() {
            Some(method) if method == methods::EXECUTE_TASK => {
                if let Some(params) = msg.params {
                    match serde_json::from_value::<ExecuteTaskParams>(params) {
                        Ok(task_params) => {
                            // Execute in a spawned task so we can handle
                            // concurrent messages (like shutdown)
                            let args_clone = Args::clone(&args);
                            tokio::spawn(async move {
                                execute_task(task_params, &args_clone).await;
                            });
                        }
                        Err(e) => {
                            if let Some(id) = &msg.id {
                                respond_error(
                                    match id {
                                        shannon_agents::JsonRpcId::Number(n) => *n,
                                        shannon_agents::JsonRpcId::String(_) => -1,
                                    },
                                    JsonRpcError::internal(e.to_string()),
                                ).await;
                            }
                        }
                    }
                }
            }
            Some(method) if method == methods::SHUTDOWN => {
                tracing::info!("Received shutdown notification, exiting");
                break;
            }
            Some(method) if method == methods::PING => {
                if let Some(id) = &msg.id {
                    let rpc_id = match id {
                        shannon_agents::JsonRpcId::Number(n) => *n,
                        shannon_agents::JsonRpcId::String(_) => -1,
                    };
                    respond(rpc_id, serde_json::json!({"status": "ok"})).await;
                }
            }
            Some(method) => {
                tracing::warn!(method = %method, "Unknown method");
                if let Some(id) = &msg.id {
                    let rpc_id = match id {
                        shannon_agents::JsonRpcId::Number(n) => *n,
                        shannon_agents::JsonRpcId::String(_) => -1,
                    };
                    respond_error(rpc_id, JsonRpcError::not_found(method)).await;
                }
            }
            None => {
                // Response to one of our requests — not expected in the
                // initial implementation but handle gracefully.
                tracing::debug!("Received response (ignoring)");
            }
        }
    }

    tracing::info!("Agent process exiting");
}
