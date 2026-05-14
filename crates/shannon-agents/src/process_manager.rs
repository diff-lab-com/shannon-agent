//! Agent process management for isolated agent execution.
//!
//! Manages agent processes as separate OS processes communicating via
//! JSON-RPC over stdin/stdout. Each agent runs in its own process for
//! crash isolation, resource boundaries, and parallel execution.

use crate::protocol::{
    self, AgentReadyParams, AgentIdleParams, ExecuteTaskParams, JsonRpcMessage,
    TaskCompleteParams, TaskProgressParams, frame_message, parse_message,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, RwLock};

/// Environment variables blocked from being passed to agent processes.
const BLOCKED_ENV: &[&str] = &[
    "LD_PRELOAD", "LD_LIBRARY_PATH", "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH", "__KMP_REGISTERED_LIBRARIES",
];

/// Status of an agent process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentProcessStatus {
    /// Process is starting up (not yet sent `agent_ready`).
    Starting,
    /// Process is ready and waiting for tasks.
    Idle,
    /// Process is executing a task.
    Busy,
    /// Process has stopped (exited or killed).
    Stopped,
    /// Process crashed and may need restart.
    Crashed,
}

/// Configuration for spawning an agent process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProcessConfig {
    /// Path to the `shannon` binary (run with `--team-agent` flag).
    pub binary_path: PathBuf,
    /// Command-line arguments to pass to the process.
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables.
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory (isolated worktree, if configured).
    pub worktree_path: Option<PathBuf>,
    /// LLM model override for this agent.
    pub model: Option<String>,
    /// System prompt for this agent.
    pub system_prompt: Option<String>,
    /// Agent name (passed as `--name` argument).
    pub agent_name: String,
    /// Permission/approval mode for the agent (e.g. "auto", "bypassPermissions").
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// If set, only these tool names are accessible to this agent.
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    /// Maximum seconds to wait for the agent to send `agent_ready` before killing it.
    /// Default: 60 seconds.
    #[serde(default = "default_startup_timeout")]
    pub startup_timeout_secs: u64,
}

fn default_startup_timeout() -> u64 { 60 }

/// Configuration for health monitoring and restart policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckConfig {
    /// Interval between health checks in seconds. Default: 30.
    #[serde(default = "default_health_interval")]
    pub check_interval_secs: u64,
    /// Timeout for ping response before marking agent unresponsive (seconds). Default: 10.
    #[serde(default = "default_ping_timeout")]
    pub ping_timeout_secs: u64,
    /// Maximum restart attempts before giving up. Default: 3.
    #[serde(default = "default_max_restarts")]
    pub max_restart_attempts: u32,
    /// Grace period before first health check after spawn (seconds). Default: 15.
    #[serde(default = "default_grace_period")]
    pub startup_grace_period_secs: u64,
    /// Timeout for graceful shutdown before force-killing (seconds). Default: 10.
    #[serde(default = "default_shutdown_timeout")]
    pub graceful_shutdown_timeout_secs: u64,
}

fn default_health_interval() -> u64 { 30 }
fn default_ping_timeout() -> u64 { 10 }
fn default_max_restarts() -> u32 { 3 }
fn default_grace_period() -> u64 { 15 }
fn default_shutdown_timeout() -> u64 { 10 }

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: default_health_interval(),
            ping_timeout_secs: default_ping_timeout(),
            max_restart_attempts: default_max_restarts(),
            startup_grace_period_secs: default_grace_period(),
            graceful_shutdown_timeout_secs: default_shutdown_timeout(),
        }
    }
}

/// A pending RPC response waiter.
struct PendingRpc {
    sender: oneshot::Sender<Result<JsonRpcMessage, String>>,
}

impl Drop for AgentHandle {
    fn drop(&mut self) {
        // Kill the child process to prevent zombies.
        // tokio::process::Child does NOT kill on drop — it only closes stdin.
        match self.child.try_wait() {
            Ok(Some(_)) => {} // already exited, no need to kill
            Ok(None) => {
                // Still running — kill it
                if let Err(e) = self.child.start_kill() {
                    tracing::warn!(agent = %self.name, "Failed to kill child process on drop: {e}");
                }
            }
            Err(e) => {
                tracing::warn!(agent = %self.name, "Failed to check child status on drop: {e}");
            }
        }
    }
}

/// Handle to a running agent process.
#[allow(dead_code)]
pub struct AgentHandle {
    /// The spawned child process.
    child: Child,
    /// stdin for sending messages to the agent.
    stdin: ChildStdin,
    /// Agent name.
    name: String,
    /// Current status.
    status: AgentProcessStatus,
    /// Pending RPC responses keyed by request ID (shared with read_loop).
    pending_rpcs: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>>,
    /// Channel for receiving events (notifications) from the agent.
    event_tx: mpsc::Sender<AgentEvent>,
    /// Kill handle sender — drop the child on shutdown.
    _kill_tx: oneshot::Sender<()>,
    /// Original config used to spawn this agent (for restart).
    config: AgentProcessConfig,
    /// Number of times this agent has been restarted.
    restart_count: u32,
    /// Time of last successful communication (health tracking).
    last_seen: Instant,
}

/// Events emitted by agent processes.
pub enum AgentEvent {
    /// Agent process is ready.
    Ready {
        agent_name: String,
        capabilities: Vec<String>,
    },
    /// Streaming progress from a task.
    Progress {
        agent_name: String,
        task_id: String,
        chunk: String,
    },
    /// Task completed (or failed).
    TaskComplete {
        agent_name: String,
        task_id: String,
        success: bool,
        output: String,
    },
    /// Agent is idle and looking for more work.
    Idle {
        agent_name: String,
        available_tasks_count: usize,
    },
    /// Agent process exited.
    ProcessExited {
        agent_name: String,
        exit_code: Option<i32>,
    },
    /// Health check failed for an agent.
    HealthCheckFailed {
        agent_name: String,
        consecutive_failures: u32,
    },
    /// Agent was automatically restarted after a crash.
    AgentRestarted {
        agent_name: String,
        restart_count: u32,
    },
    /// Agent sent an RPC request that needs a coordinator response.
    RpcRequest {
        agent_name: String,
        request_id: i64,
        method: String,
        params: serde_json::Value,
    },
}

/// Manages agent process lifecycle.
pub struct AgentProcessManager {
    /// Running agent handles, keyed by agent name.
    agents: Arc<RwLock<HashMap<String, AgentHandle>>>,
    /// JoinHandles for spawned background tasks (timeout guards, watchers).
    /// Aborted on drop to prevent leaked tasks.
    task_handles: Arc<std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>>,
    /// Next RPC request ID.
    next_rpc_id: AtomicU64,
    /// Event receiver — consume events via `recv()`.
    event_rx: mpsc::Receiver<AgentEvent>,
    /// Event sender — cloned for each agent.
    event_tx: mpsc::Sender<AgentEvent>,
    /// Health monitor task handle — aborted on drop.
    health_monitor_handle: std::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl Drop for AgentProcessManager {
    fn drop(&mut self) {
        if let Some(handle) = self.health_monitor_handle.lock().ok().and_then(|mut g| g.take()) {
            handle.abort();
        }
        // Abort all tracked background tasks (timeout guards, watchers)
        if let Ok(mut handles) = self.task_handles.lock() {
            for handle in handles.drain(..) {
                handle.abort();
            }
        }
    }
}

impl Default for AgentProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentProcessManager {
    /// Create a new process manager.
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            task_handles: Arc::new(std::sync::Mutex::new(Vec::new())),
            next_rpc_id: AtomicU64::new(1),
            event_rx,
            event_tx,
            health_monitor_handle: std::sync::Mutex::new(None),
        }
    }

    /// Allocate a new RPC request ID.
    fn next_id(&self) -> i64 {
        self.next_rpc_id.fetch_add(1, Ordering::Relaxed) as i64
    }

    /// Track a spawned background task for cleanup on drop.
    fn track_task(&self, handle: tokio::task::JoinHandle<()>) {
        if let Ok(mut handles) = self.task_handles.lock() {
            // Prune completed handles to avoid unbounded growth
            handles.retain(|h| !h.is_finished());
            handles.push(handle);
        }
    }

    /// Spawn a new agent process.
    ///
    /// The process is started and reads JSON-RPC from stdin, writes to stdout.
    /// Logs go to stderr (captured by the coordinator at trace level).
    pub async fn spawn_agent(
        &self,
        config: AgentProcessConfig,
    ) -> Result<String, AgentProcessError> {
        let name = config.agent_name.clone();
        let startup_timeout_secs = config.startup_timeout_secs;

        // Validate binary path exists and is executable
        let binary_path = std::path::Path::new(&config.binary_path);
        if !binary_path.exists() {
            return Err(AgentProcessError::AgentNotFound(
                format!("Agent binary not found: {}", binary_path.display())
            ));
        }

        // Build command — uses `shannon --team-agent` to reuse the main binary
        let mut cmd = Command::new(&config.binary_path);
        cmd.arg("--team-agent");
        cmd.arg("--name").arg(&name);
        if let Some(ref model) = config.model {
            cmd.arg("--model").arg(model);
        }
        if let Some(ref prompt) = config.system_prompt {
            cmd.arg("--system-prompt").arg(prompt);
        }
        if let Some(ref path) = config.worktree_path {
            cmd.arg("--workdir").arg(path);
            cmd.current_dir(path);
        }
        if let Some(ref mode) = config.permission_mode {
            cmd.arg("--permission-mode").arg(mode);
        }
        if let Some(ref tools) = config.allowed_tools {
            cmd.arg("--allowed-tools").arg(tools.join(","));
        }
        cmd.args(&config.args);
        // Filter dangerous env vars that could enable code injection
        for (key, value) in &config.env {
            if !BLOCKED_ENV.contains(&key.as_str()) {
                cmd.env(key, value);
            }
        }
        cmd.stdout(std::process::Stdio::piped())
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        // Kill switch
        let (kill_tx, kill_rx) = oneshot::channel::<()>();

        let mut child = cmd.spawn().map_err(|e| AgentProcessError::SpawnFailed {
            agent: name.clone(),
            source: e,
        })?;

        let stdin = child.stdin.take()
            .ok_or_else(|| AgentProcessError::SpawnFailed {
                agent: name.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "stdin not captured",
                ),
            })?;
        let stdout = child.stdout.take()
            .ok_or_else(|| AgentProcessError::SpawnFailed {
                agent: name.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "stdout not captured",
                ),
            })?;

        let event_tx = self.event_tx.clone();
        let agent_name_for_reader = name.clone();
        let pending_rpcs = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let rpc_map_for_reader = pending_rpcs.clone();

        // Spawn a reader task that reads lines from stdout and dispatches
        let read_handle = tokio::spawn(async move {
            Self::read_loop(stdout, event_tx, agent_name_for_reader, rpc_map_for_reader).await;
        });
        self.track_task(read_handle);

        // Spawn a watcher for process exit + kill signal
        let event_tx_exit = self.event_tx.clone();
        let name_exit = name.clone();
        let watcher_handle = tokio::spawn(async move {
            // Wait for kill signal or just let it drop
            let _ = kill_rx.await;
            // The child will be killed when AgentHandle is dropped
            let _ = event_tx_exit.send(AgentEvent::ProcessExited {
                agent_name: name_exit.clone(),
                exit_code: None,
            }).await;
        });
        self.track_task(watcher_handle);

        let handle = AgentHandle {
            child,
            stdin,
            name: name.clone(),
            status: AgentProcessStatus::Starting,
            pending_rpcs,
            event_tx: self.event_tx.clone(),
            _kill_tx: kill_tx,
            config,
            restart_count: 0,
            last_seen: Instant::now(),
        };

        self.agents.write().await.insert(name.clone(), handle);

        // Startup timeout guard: kill the agent if it stays in Starting state too long
        let timeout_name = name.clone();
        let timeout_agents = self.agents.clone();
        let timeout_event_tx = self.event_tx.clone();
        let startup_timeout = Duration::from_secs(startup_timeout_secs);
        let timeout_handle = tokio::spawn(async move {
            tokio::time::sleep(startup_timeout).await;
            let mut agents = timeout_agents.write().await;
            if let Some(handle) = agents.get_mut(&timeout_name) {
                if handle.status == AgentProcessStatus::Starting {
                    tracing::warn!(agent = %timeout_name, "Startup timeout exceeded, killing agent");
                    let _ = handle.child.start_kill();
                    handle.status = AgentProcessStatus::Crashed;
                    let _ = timeout_event_tx.send(AgentEvent::ProcessExited {
                        agent_name: timeout_name,
                        exit_code: None,
                    }).await;
                }
            }
        });
        self.track_task(timeout_handle);

        tracing::info!(agent = %name, "Spawned agent process");
        Ok(name)
    }

    /// Send a JSON-RPC request to an agent and wait for the response.
    pub async fn send_request(
        &self,
        agent_name: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<JsonRpcMessage, AgentProcessError> {
        let rpc_id = self.next_id();
        let msg = JsonRpcMessage::request(method, params, rpc_id);

        let mut agents = self.agents.write().await;
        let handle = agents.get_mut(agent_name)
            .ok_or_else(|| AgentProcessError::AgentNotFound(agent_name.to_string()))?;

        let (tx, rx) = oneshot::channel();
        {
            let mut rpcs = handle.pending_rpcs.lock().unwrap();
            rpcs.insert(rpc_id, PendingRpc { sender: tx });
        }

        let line = frame_message(&msg)
            .map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        handle.stdin.write_all(line.as_bytes()).await
            .map_err(AgentProcessError::Io)?;
        handle.stdin.flush().await
            .map_err(AgentProcessError::Io)?;

        drop(agents);

        // Wait for response
        rx.await.map_err(|_| AgentProcessError::Protocol(
            format!("Agent '{agent_name}' dropped response channel for RPC {rpc_id}")
        ))?.map_err(AgentProcessError::Protocol)
    }

    /// Send a notification (no response expected) to an agent.
    pub async fn send_notification(
        &self,
        agent_name: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), AgentProcessError> {
        let msg = JsonRpcMessage::notification(method, params);

        let mut agents = self.agents.write().await;
        let handle = agents.get_mut(agent_name)
            .ok_or_else(|| AgentProcessError::AgentNotFound(agent_name.to_string()))?;

        let line = frame_message(&msg)
            .map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        handle.stdin.write_all(line.as_bytes()).await
            .map_err(AgentProcessError::Io)?;
        handle.stdin.flush().await
            .map_err(AgentProcessError::Io)?;

        Ok(())
    }

    /// Send an `execute_task` request to an agent.
    pub async fn execute_task(
        &self,
        agent_name: &str,
        params: ExecuteTaskParams,
    ) -> Result<(), AgentProcessError> {
        let params_value = serde_json::to_value(&params)
            .map_err(|e| AgentProcessError::Protocol(e.to_string()))?;

        // execute_task is a notification (fire-and-forget); agent sends back
        // task_progress and task_complete notifications.
        self.send_notification(agent_name, protocol::methods::EXECUTE_TASK, params_value).await
    }

    /// Send a `shutdown` notification to an agent.
    pub async fn shutdown_agent(&self, agent_name: &str, reason: &str) -> Result<(), AgentProcessError> {
        let params = serde_json::to_value(ShutdownParamsWrapper { reason: reason.to_string() })
            .map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        self.send_notification(agent_name, protocol::methods::SHUTDOWN, params).await
    }

    /// Kill an agent process immediately.
    pub async fn kill_agent(&self, agent_name: &str) -> Result<(), AgentProcessError> {
        let mut agents = self.agents.write().await;
        if let Some(mut handle) = agents.remove(agent_name) {
            let _ = handle.child.kill().await;
            tracing::info!(agent = %agent_name, "Killed agent process");
            Ok(())
        } else {
            Err(AgentProcessError::AgentNotFound(agent_name.to_string()))
        }
    }

    /// Gracefully shut down an agent: send `shutdown` notification, wait for
    /// the process to exit, then force-kill after timeout.
    pub async fn graceful_shutdown_agent(
        &self,
        agent_name: &str,
        timeout: Duration,
    ) -> Result<(), AgentProcessError> {
        // Send shutdown notification (best-effort)
        let _ = self.shutdown_agent(agent_name, "coordinator shutting down").await;

        // Wait for the process to exit
        let deadline = Instant::now() + timeout;
        loop {
            let exited = {
                let mut agents = self.agents.write().await;
                match agents.get_mut(agent_name) {
                    Some(handle) => {
                        matches!(handle.child.try_wait(), Ok(Some(_)))
                    }
                    None => true,
                }
            };

            if exited {
                tracing::info!(agent = %agent_name, "Agent exited gracefully");
                return Ok(());
            }

            if Instant::now() >= deadline {
                tracing::warn!(
                    agent = %agent_name,
                    ?timeout,
                    "Agent did not exit within timeout, force-killing"
                );
                return self.kill_agent(agent_name).await;
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// Restart a crashed agent using its stored configuration.
    ///
    /// Removes the old handle, respawns using the original config, and
    /// increments the restart counter.
    pub async fn restart_agent(
        &self,
        agent_name: &str,
    ) -> Result<String, AgentProcessError> {
        // Extract the stored config and restart count
        let config = {
            let mut agents = self.agents.write().await;
            match agents.remove(agent_name) {
                Some(handle) => {
                    let restart_count = handle.restart_count + 1;
                    let mut config = handle.config.clone();
                    config.agent_name = handle.name.clone();
                    (config, restart_count)
                }
                None => {
                    return Err(AgentProcessError::AgentNotFound(agent_name.to_string()));
                }
            }
        };

        let (config, restart_count) = config;

        tracing::info!(
            agent = %agent_name,
            restart_count,
            "Restarting agent process"
        );

        // Respawn with same config
        let name = self.spawn_agent(config).await?;

        // Update restart count
        {
            let mut agents = self.agents.write().await;
            if let Some(handle) = agents.get_mut(&name) {
                handle.restart_count = restart_count;
            }
        }

        let _ = self.event_tx.send(AgentEvent::AgentRestarted {
            agent_name: name.clone(),
            restart_count,
        }).await;

        Ok(name)
    }

    /// Start a background health monitoring task that periodically pings
    /// agents and auto-restarts crashed ones.
    pub fn start_health_monitor(&self, health_config: HealthCheckConfig) {
        let agents = self.agents.clone();
        let event_tx = self.event_tx.clone();
        let check_interval = Duration::from_secs(health_config.check_interval_secs);
        let max_restarts = health_config.max_restart_attempts;
        let grace_period = Duration::from_secs(health_config.startup_grace_period_secs);

        let handle = tokio::spawn(async move {
            // Wait for initial grace period
            tokio::time::sleep(grace_period).await;

            let mut interval = tokio::time::interval(check_interval);
            // Track consecutive ping failures per agent
            let mut failures: HashMap<String, u32> = HashMap::new();

            loop {
                interval.tick().await;

                let agent_names: Vec<String> = {
                    let agents_guard = agents.read().await;
                    agents_guard.keys().cloned().collect()
                };

                for agent_name in agent_names {
                    // Use write lock since try_wait() needs &mut Child
                    let mut agents_guard = agents.write().await;
                    let Some(handle) = agents_guard.get_mut(&agent_name) else {
                        continue;
                    };

                    // Skip agents that are already stopped/crashed
                    if matches!(handle.status, AgentProcessStatus::Stopped | AgentProcessStatus::Crashed) {
                        continue;
                    }

                    // Check if process is still alive
                    let alive = !matches!(handle.child.try_wait(), Ok(Some(_)));
                    drop(agents_guard);

                    if !alive {
                        tracing::warn!(agent = %agent_name, "Agent process is dead");
                        let failure_count = failures.entry(agent_name.clone()).or_insert(0);
                        *failure_count += 1;

                        let _ = event_tx.send(AgentEvent::HealthCheckFailed {
                            agent_name: agent_name.clone(),
                            consecutive_failures: *failure_count,
                        }).await;

                        // Mark as crashed
                        {
                            let mut agents_guard = agents.write().await;
                            if let Some(handle) = agents_guard.get_mut(&agent_name) {
                                handle.status = AgentProcessStatus::Crashed;
                            }
                        }

                        // Attempt auto-restart if under limit
                        if *failure_count <= max_restarts {
                            tracing::info!(
                                agent = %agent_name,
                                attempt = *failure_count,
                                max = max_restarts,
                                "Attempting auto-restart"
                            );
                            // Can't call self.restart_agent since we don't have &self
                            // Instead, manually respawn using the stored config
                            let mut agents_guard = agents.write().await;
                            if let Some(old_handle) = agents_guard.remove(&agent_name) {
                                let restart_count = old_handle.restart_count + 1;
                                let config = old_handle.config.clone();
                                drop(agents_guard);

                                // Build and spawn new process
                                let name = config.agent_name.clone();
                                let mut cmd = Command::new(&config.binary_path);
                                cmd.arg("--team-agent");
                                cmd.arg("--name").arg(&name);
                                if let Some(ref model) = config.model {
                                    cmd.arg("--model").arg(model);
                                }
                                if let Some(ref prompt) = config.system_prompt {
                                    cmd.arg("--system-prompt").arg(prompt);
                                }
                                if let Some(ref path) = config.worktree_path {
                                    cmd.arg("--workdir").arg(path);
                                    cmd.current_dir(path);
                                }
                                if let Some(ref mode) = config.permission_mode {
                                    cmd.arg("--permission-mode").arg(mode);
                                }
                                cmd.args(&config.args);
                                // Apply same env filtering as spawn path
                                for (key, value) in &config.env {
                                    if !BLOCKED_ENV.contains(&key.as_str()) {
                                        cmd.env(key, value);
                                    }
                                }
                                cmd.stdout(std::process::Stdio::piped())
                                    .stdin(std::process::Stdio::piped())
                                    .stderr(std::process::Stdio::piped());

                                let (kill_tx, kill_rx) = oneshot::channel::<()>();

                                match cmd.spawn() {
                                    Ok(mut child) => {
                                        let stdin = child.stdin.take();
                                        let stdout = child.stdout.take();

                                        match (stdin, stdout) {
                                            (Some(stdin), Some(stdout)) => {
                                                let evt_clone = event_tx.clone();
                                                let name_reader = name.clone();
                                                let rpc_map = Arc::new(std::sync::Mutex::new(HashMap::new()));
                                                let rpc_map_reader = rpc_map.clone();
                                                tokio::spawn(async move {
                                                    Self::read_loop(stdout, evt_clone, name_reader, rpc_map_reader).await;
                                                });

                                                let evt_exit = event_tx.clone();
                                                let name_exit = name.clone();
                                                tokio::spawn(async move {
                                                    let _ = kill_rx.await;
                                                    let _ = evt_exit.send(AgentEvent::ProcessExited {
                                                        agent_name: name_exit,
                                                        exit_code: None,
                                                    }).await;
                                                });

                                                let handle = AgentHandle {
                                                    child,
                                                    stdin,
                                                    name: name.clone(),
                                                    status: AgentProcessStatus::Starting,
                                                    pending_rpcs: rpc_map,
                                                    event_tx: event_tx.clone(),
                                                    _kill_tx: kill_tx,
                                                    config,
                                                    restart_count,
                                                    last_seen: Instant::now(),
                                                };

                                                let mut agents_guard = agents.write().await;
                                                agents_guard.insert(name.clone(), handle);

                                                let _ = event_tx.send(AgentEvent::AgentRestarted {
                                                    agent_name: name.clone(),
                                                    restart_count,
                                                }).await;

                                                tracing::info!(
                                                    agent = %name,
                                                    restart_count,
                                                    "Auto-restarted agent"
                                                );
                                            }
                                            _ => {
                                                tracing::error!(
                                                    agent = %name,
                                                    "Failed to capture stdin/stdout during restart"
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            agent = %name,
                                            error = %e,
                                            "Failed to respawn agent"
                                        );
                                    }
                                }
                            }
                        } else {
                            tracing::error!(
                                agent = %agent_name,
                                failures = *failure_count,
                                max = max_restarts,
                                "Exceeded max restart attempts, giving up"
                            );
                        }
                    } else {
                        // Agent is alive, reset failure counter
                        if let Some(f) = failures.get_mut(&agent_name) {
                            *f = 0;
                        }
                        // Update last_seen
                        let mut agents_guard = agents.write().await;
                        if let Some(handle) = agents_guard.get_mut(&agent_name) {
                            handle.last_seen = Instant::now();
                        }
                    }
                }
            }
        });
        self.health_monitor_handle.lock().unwrap().replace(handle);
    }

    /// Get the status of an agent.
    pub async fn agent_status(&self, agent_name: &str) -> Option<AgentProcessStatus> {
        let agents = self.agents.read().await;
        agents.get(agent_name).map(|h| h.status)
    }

    /// List all running agent names.
    pub async fn running_agents(&self) -> Vec<String> {
        let agents = self.agents.read().await;
        agents.keys().cloned().collect()
    }

    /// Receive the next event from any agent process (non-blocking).
    pub async fn recv_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.recv().await
    }

    /// Try to receive an event without blocking.
    pub fn try_recv_event(&mut self) -> Option<AgentEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Take the event receiver out, leaving a closed channel in its place.
    ///
    /// This allows the coordinator to own the event receiver in a separate task
    /// while the process manager itself remains usable for command operations
    /// (spawn, execute_task, shutdown).
    pub fn take_event_receiver(&mut self) -> mpsc::Receiver<AgentEvent> {
        let (_, rx) = mpsc::channel(1);
        std::mem::replace(&mut self.event_rx, rx)
    }

    /// Read loop: reads lines from agent stdout, dispatches events and RPC responses.
    async fn read_loop(
        stdout: ChildStdout,
        event_tx: mpsc::Sender<AgentEvent>,
        agent_name: String,
        pending_rpcs: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>>,
    ) {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let msg = match parse_message(&line) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(
                        agent = %agent_name,
                        error = %e,
                        line = %line,
                        "Failed to parse JSON-RPC from agent"
                    );
                    continue;
                }
            };

            // Dispatch based on method
            if let Some(method) = msg.method() {
                match method {
                    // RPC requests from agent (have an id, expect a response)
                    protocol::methods::CREATE_TASK
                    | protocol::methods::UPDATE_TASK
                    | protocol::methods::GET_TASK
                    | protocol::methods::TEAM_MANIFEST
                    | protocol::methods::LIST_TASKS
                    | protocol::methods::CLAIM_TASK
                    | protocol::methods::DISBAND_TEAM
                    | protocol::methods::ADD_AGENT => {
                        let request_id = msg.id.as_ref().and_then(|id| match id {
                            protocol::JsonRpcId::Number(n) => Some(*n),
                            _ => None,
                        });
                        if let Some(request_id) = request_id {
                            let _ = event_tx.send(AgentEvent::RpcRequest {
                                agent_name: agent_name.clone(),
                                request_id,
                                method: method.to_string(),
                                params: msg.params.clone().unwrap_or_default(),
                            }).await;
                        }
                    }
                    protocol::methods::AGENT_READY => {
                        if let Ok(params) = serde_json::from_value::<AgentReadyParams>(
                            msg.params.unwrap_or_default()
                        ) {
                            let _ = event_tx.send(AgentEvent::Ready {
                                agent_name: params.agent_name.clone(),
                                capabilities: params.capabilities,
                            }).await;
                        }
                    }
                    protocol::methods::TASK_PROGRESS => {
                        if let Ok(params) = serde_json::from_value::<TaskProgressParams>(
                            msg.params.unwrap_or_default()
                        ) {
                            let _ = event_tx.send(AgentEvent::Progress {
                                agent_name: agent_name.clone(),
                                task_id: params.task_id,
                                chunk: params.chunk,
                            }).await;
                        }
                    }
                    protocol::methods::TASK_COMPLETE => {
                        if let Ok(params) = serde_json::from_value::<TaskCompleteParams>(
                            msg.params.unwrap_or_default()
                        ) {
                            let _ = event_tx.send(AgentEvent::TaskComplete {
                                agent_name: agent_name.clone(),
                                task_id: params.task_id,
                                success: params.success,
                                output: params.output,
                            }).await;
                        }
                    }
                    protocol::methods::AGENT_IDLE => {
                        if let Ok(params) = serde_json::from_value::<AgentIdleParams>(
                            msg.params.unwrap_or_default()
                        ) {
                            let _ = event_tx.send(AgentEvent::Idle {
                                agent_name: params.agent_name,
                                available_tasks_count: params.available_tasks_count,
                            }).await;
                        }
                    }
                    other => {
                        tracing::debug!(
                            agent = %agent_name,
                            method = %other,
                            "Unhandled method from agent"
                        );
                    }
                }
            } else if let Some(rpc_id) = msg.id.as_ref().and_then(|id| match id {
                protocol::JsonRpcId::Number(n) => Some(*n),
                _ => None,
            }) {
                // JSON-RPC response (has id, no method) — dispatch to pending waiter
                if let Some(pending) = pending_rpcs.lock().unwrap().remove(&rpc_id) {
                    let _ = pending.sender.send(Ok(msg));
                } else {
                    tracing::debug!(
                        agent = %agent_name,
                        rpc_id,
                        "Received response for unknown RPC request"
                    );
                }
            }
        }

        tracing::info!(agent = %agent_name, "Agent stdout closed");
        let _ = event_tx.send(AgentEvent::ProcessExited {
            agent_name,
            exit_code: None,
        }).await;
    }

    /// Shut down all agents.
    pub async fn shutdown_all(&self) {
        let mut agents = self.agents.write().await;
        for (name, mut handle) in agents.drain() {
            tracing::info!(agent = %name, "Shutting down agent process");
            let _ = handle.child.kill().await;
        }
    }

    /// Send a JSON-RPC response back to an agent process via its stdin.
    pub async fn send_rpc_response(
        &self,
        agent_name: &str,
        request_id: i64,
        result: serde_json::Value,
    ) -> Result<(), AgentProcessError> {
        let mut agents = self.agents.write().await;
        let handle = agents.get_mut(agent_name).ok_or_else(|| {
            AgentProcessError::AgentNotFound(agent_name.to_string())
        })?;

        let response = JsonRpcMessage::response(
            protocol::JsonRpcId::Number(request_id),
            result,
        );
        let line = frame_message(&response).map_err(|e| AgentProcessError::SpawnFailed {
            agent: agent_name.to_string(),
            source: std::io::Error::other(e.to_string()),
        })?;

        use tokio::io::AsyncWriteExt;
        handle.stdin.write_all(line.as_bytes()).await.map_err(|e| {
            AgentProcessError::SpawnFailed {
                agent: agent_name.to_string(),
                source: e,
            }
        })?;
        handle.stdin.flush().await.map_err(|e| {
            AgentProcessError::SpawnFailed {
                agent: agent_name.to_string(),
                source: e,
            }
        })?;
        Ok(())
    }
}

/// Wrapper for shutdown params (avoids name collision with protocol::ShutdownParams).
#[derive(Serialize, Deserialize)]
struct ShutdownParamsWrapper {
    reason: String,
}

/// Errors from agent process operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentProcessError {
    #[error("Agent process spawn failed for '{agent}': {source}")]
    SpawnFailed {
        agent: String,
        source: std::io::Error,
    },
    #[error("Agent not found: {0}")]
    AgentNotFound(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_manager_creation() {
        let _mgr = AgentProcessManager::new();
    }

    #[tokio::test]
    async fn test_spawn_nonexistent_binary() {
        let mgr = AgentProcessManager::new();
        let config = AgentProcessConfig {
            binary_path: PathBuf::from("/nonexistent/shannon"),
            args: vec![],
            env: HashMap::new(),
            worktree_path: None,
            model: None,
            system_prompt: None,
            agent_name: "test-agent".to_string(),
            permission_mode: None,
            allowed_tools: None,
            startup_timeout_secs: 60,
        };
        let result = mgr.spawn_agent(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_agent_not_found_for_request() {
        let mgr = AgentProcessManager::new();
        let result = mgr.send_request(
            "nonexistent",
            "ping",
            serde_json::Value::Null,
        ).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_agent_not_found_for_kill() {
        let mgr = AgentProcessManager::new();
        let result = mgr.kill_agent("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_running_agents_empty() {
        let mgr = AgentProcessManager::new();
        let agents = mgr.running_agents().await;
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn test_agent_status_nonexistent() {
        let mgr = AgentProcessManager::new();
        let status = mgr.agent_status("nonexistent").await;
        assert!(status.is_none());
    }
}
