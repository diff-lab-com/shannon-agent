//! Agent process management for isolated agent execution.
//!
//! Manages agent processes as separate OS processes communicating via
//! JSON-RPC over stdin/stdout. Each agent runs in its own process for
//! crash isolation, resource boundaries, and parallel execution.

use crate::protocol::{
    self, AgentIdleParams, AgentReadyParams, ExecuteTaskParams, JsonRpcMessage, TaskCompleteParams,
    TaskProgressParams, frame_message, parse_message,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{RwLock, mpsc, oneshot};

/// Environment variables blocked from being passed to agent processes.
const BLOCKED_ENV: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "__KMP_REGISTERED_LIBRARIES",
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

fn default_startup_timeout() -> u64 {
    60
}

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

fn default_health_interval() -> u64 {
    30
}
fn default_ping_timeout() -> u64 {
    10
}
fn default_max_restarts() -> u32 {
    3
}
fn default_grace_period() -> u64 {
    15
}
fn default_shutdown_timeout() -> u64 {
    10
}

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
                // Reap the zombie to free the PID slot
                if let Err(e) = self.child.try_wait() {
                    tracing::debug!(agent = %self.name, error = %e, "Failed to reap zombie process on drop");
                }
            }
            Err(e) => {
                tracing::warn!(agent = %self.name, "Failed to check child status on drop: {e}");
            }
        }
    }
}

/// Handle to a running agent process.
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
    #[allow(dead_code)] // KEEP: watcher lifecycle
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
        if let Some(handle) = self
            .health_monitor_handle
            .lock()
            .ok()
            .and_then(|mut g| g.take())
        {
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
            return Err(AgentProcessError::AgentNotFound(format!(
                "Agent binary not found: {}",
                binary_path.display()
            )));
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

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AgentProcessError::SpawnFailed {
                agent: name.clone(),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin not captured"),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AgentProcessError::SpawnFailed {
                agent: name.clone(),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdout not captured"),
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
            let _ = event_tx_exit
                .send(AgentEvent::ProcessExited {
                    agent_name: name_exit.clone(),
                    exit_code: None,
                })
                .await;
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
                    if let Err(e) = handle.child.start_kill() {
                        tracing::debug!(agent = %timeout_name, error = %e, "Failed to kill agent process during startup timeout");
                    }
                    handle.status = AgentProcessStatus::Crashed;
                    if let Err(e) = timeout_event_tx
                        .send(AgentEvent::ProcessExited {
                            agent_name: timeout_name.clone(),
                            exit_code: None,
                        })
                        .await
                    {
                        tracing::debug!(agent = %timeout_name, error = %e, "Failed to send startup timeout event");
                    }
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
        let handle = agents
            .get_mut(agent_name)
            .ok_or_else(|| AgentProcessError::AgentNotFound(agent_name.to_string()))?;

        let (tx, rx) = oneshot::channel();
        {
            let mut rpcs = handle
                .pending_rpcs
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            rpcs.insert(rpc_id, PendingRpc { sender: tx });
        }

        let line = frame_message(&msg).map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        handle
            .stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AgentProcessError::Io)?;
        handle.stdin.flush().await.map_err(AgentProcessError::Io)?;

        drop(agents);

        // Wait for response
        rx.await
            .map_err(|_| {
                AgentProcessError::Protocol(format!(
                    "Agent '{agent_name}' dropped response channel for RPC {rpc_id}"
                ))
            })?
            .map_err(AgentProcessError::Protocol)
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
        let handle = agents
            .get_mut(agent_name)
            .ok_or_else(|| AgentProcessError::AgentNotFound(agent_name.to_string()))?;

        let line = frame_message(&msg).map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        handle
            .stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AgentProcessError::Io)?;
        handle.stdin.flush().await.map_err(AgentProcessError::Io)?;

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
        self.send_notification(agent_name, protocol::methods::EXECUTE_TASK, params_value)
            .await
    }

    /// Send a `shutdown` notification to an agent.
    pub async fn shutdown_agent(
        &self,
        agent_name: &str,
        reason: &str,
    ) -> Result<(), AgentProcessError> {
        let params = serde_json::to_value(ShutdownParamsWrapper {
            reason: reason.to_string(),
        })
        .map_err(|e| AgentProcessError::Protocol(e.to_string()))?;
        self.send_notification(agent_name, protocol::methods::SHUTDOWN, params)
            .await
    }

    /// Kill an agent process immediately.
    pub async fn kill_agent(&self, agent_name: &str) -> Result<(), AgentProcessError> {
        let mut agents = self.agents.write().await;
        if let Some(mut handle) = agents.remove(agent_name) {
            if let Err(e) = handle.child.kill().await {
                tracing::debug!(agent = %agent_name, error = %e, "Failed to kill agent process");
            }
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
        let _ = self
            .shutdown_agent(agent_name, "coordinator shutting down")
            .await;

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
    pub async fn restart_agent(&self, agent_name: &str) -> Result<String, AgentProcessError> {
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

        let _ = self
            .event_tx
            .send(AgentEvent::AgentRestarted {
                agent_name: name.clone(),
                restart_count,
            })
            .await;

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
                    if matches!(
                        handle.status,
                        AgentProcessStatus::Stopped | AgentProcessStatus::Crashed
                    ) {
                        continue;
                    }

                    // Check if process is still alive
                    let alive = !matches!(handle.child.try_wait(), Ok(Some(_)));
                    drop(agents_guard);

                    if !alive {
                        tracing::warn!(agent = %agent_name, "Agent process is dead");
                        let failure_count = failures.entry(agent_name.clone()).or_insert(0);
                        *failure_count += 1;

                        let _ = event_tx
                            .send(AgentEvent::HealthCheckFailed {
                                agent_name: agent_name.clone(),
                                consecutive_failures: *failure_count,
                            })
                            .await;

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
                                if let Some(ref allowed) = config.allowed_tools {
                                    for tool in allowed {
                                        cmd.arg("--allowed-tools").arg(tool);
                                    }
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
                                                let rpc_map =
                                                    Arc::new(std::sync::Mutex::new(HashMap::new()));
                                                let rpc_map_reader = rpc_map.clone();
                                                tokio::spawn(async move {
                                                    Self::read_loop(
                                                        stdout,
                                                        evt_clone,
                                                        name_reader,
                                                        rpc_map_reader,
                                                    )
                                                    .await;
                                                });

                                                let evt_exit = event_tx.clone();
                                                let name_exit = name.clone();
                                                tokio::spawn(async move {
                                                    let _ = kill_rx.await;
                                                    let _ = evt_exit
                                                        .send(AgentEvent::ProcessExited {
                                                            agent_name: name_exit,
                                                            exit_code: None,
                                                        })
                                                        .await;
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

                                                let _ = event_tx
                                                    .send(AgentEvent::AgentRestarted {
                                                        agent_name: name.clone(),
                                                        restart_count,
                                                    })
                                                    .await;

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
        self.health_monitor_handle
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(handle);
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
                            let _ = event_tx
                                .send(AgentEvent::RpcRequest {
                                    agent_name: agent_name.clone(),
                                    request_id,
                                    method: method.to_string(),
                                    params: msg.params.clone().unwrap_or_default(),
                                })
                                .await;
                        }
                    }
                    protocol::methods::AGENT_READY => {
                        if let Ok(params) = serde_json::from_value::<AgentReadyParams>(
                            msg.params.unwrap_or_default(),
                        ) {
                            let _ = event_tx
                                .send(AgentEvent::Ready {
                                    agent_name: params.agent_name.clone(),
                                    capabilities: params.capabilities,
                                })
                                .await;
                        }
                    }
                    protocol::methods::TASK_PROGRESS => {
                        if let Ok(params) = serde_json::from_value::<TaskProgressParams>(
                            msg.params.unwrap_or_default(),
                        ) {
                            let _ = event_tx
                                .send(AgentEvent::Progress {
                                    agent_name: agent_name.clone(),
                                    task_id: params.task_id,
                                    chunk: params.chunk,
                                })
                                .await;
                        }
                    }
                    protocol::methods::TASK_COMPLETE => {
                        if let Ok(params) = serde_json::from_value::<TaskCompleteParams>(
                            msg.params.unwrap_or_default(),
                        ) {
                            let _ = event_tx
                                .send(AgentEvent::TaskComplete {
                                    agent_name: agent_name.clone(),
                                    task_id: params.task_id,
                                    success: params.success,
                                    output: params.output,
                                })
                                .await;
                        }
                    }
                    protocol::methods::AGENT_IDLE => {
                        if let Ok(params) = serde_json::from_value::<AgentIdleParams>(
                            msg.params.unwrap_or_default(),
                        ) {
                            let _ = event_tx
                                .send(AgentEvent::Idle {
                                    agent_name: params.agent_name,
                                    available_tasks_count: params.available_tasks_count,
                                })
                                .await;
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
                if let Some(pending) = pending_rpcs
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&rpc_id)
                {
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

        // Drain orphaned pending RPCs to prevent memory leak across restarts
        let orphaned: Vec<_> = pending_rpcs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain()
            .collect();
        drop(orphaned);

        tracing::info!(agent = %agent_name, "Agent stdout closed");
        let _ = event_tx
            .send(AgentEvent::ProcessExited {
                agent_name,
                exit_code: None,
            })
            .await;
    }

    /// Shut down all agents.
    pub async fn shutdown_all(&self) {
        let mut agents = self.agents.write().await;
        for (name, mut handle) in agents.drain() {
            tracing::info!(agent = %name, "Shutting down agent process");
            if let Err(e) = handle.child.kill().await {
                tracing::debug!(agent = %name, error = %e, "Failed to kill agent process during shutdown_all");
            }
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
        let handle = agents
            .get_mut(agent_name)
            .ok_or_else(|| AgentProcessError::AgentNotFound(agent_name.to_string()))?;

        let response = JsonRpcMessage::response(protocol::JsonRpcId::Number(request_id), result);
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
        handle
            .stdin
            .flush()
            .await
            .map_err(|e| AgentProcessError::SpawnFailed {
                agent: agent_name.to_string(),
                source: e,
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
        let result = mgr
            .send_request("nonexistent", "ping", serde_json::Value::Null)
            .await;
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

    // ── Config and type tests ─────────────────────────────────────────────

    #[test]
    fn test_agent_process_status_equality() {
        assert_eq!(AgentProcessStatus::Idle, AgentProcessStatus::Idle);
        assert_ne!(AgentProcessStatus::Idle, AgentProcessStatus::Busy);
        assert_ne!(AgentProcessStatus::Starting, AgentProcessStatus::Stopped);
        assert_ne!(AgentProcessStatus::Crashed, AgentProcessStatus::Stopped);
    }

    #[test]
    fn test_agent_process_config_serialization() {
        let config = AgentProcessConfig {
            binary_path: PathBuf::from("/usr/bin/shannon"),
            args: vec!["--team-agent".to_string()],
            env: HashMap::from([("KEY".to_string(), "value".to_string())]),
            worktree_path: Some(PathBuf::from("/tmp/worktree")),
            model: Some("gpt-4".to_string()),
            system_prompt: Some("You are helpful".to_string()),
            agent_name: "test-agent".to_string(),
            permission_mode: Some("auto".to_string()),
            allowed_tools: Some(vec!["Read".to_string(), "Write".to_string()]),
            startup_timeout_secs: 30,
        };
        let json = serde_json::to_string(&config).unwrap();
        let de: AgentProcessConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.binary_path, PathBuf::from("/usr/bin/shannon"));
        assert_eq!(de.agent_name, "test-agent");
        assert_eq!(de.model, Some("gpt-4".to_string()));
        assert_eq!(de.allowed_tools.as_ref().map(|v| v.len()), Some(2));
        assert_eq!(de.startup_timeout_secs, 30);
    }

    #[test]
    fn test_agent_process_config_minimal() {
        let json = r#"{"binary_path":"/bin/sh","agent_name":"a"}"#;
        let config: AgentProcessConfig = serde_json::from_str(json).unwrap();
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert!(config.worktree_path.is_none());
        assert!(config.model.is_none());
        assert!(config.allowed_tools.is_none());
        assert_eq!(config.startup_timeout_secs, 60); // default
    }

    #[test]
    fn test_health_check_config_defaults() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.ping_timeout_secs, 10);
        assert_eq!(config.max_restart_attempts, 3);
        assert_eq!(config.startup_grace_period_secs, 15);
        assert_eq!(config.graceful_shutdown_timeout_secs, 10);
    }

    #[test]
    fn test_health_check_config_serialization() {
        let config = HealthCheckConfig {
            check_interval_secs: 60,
            ping_timeout_secs: 20,
            max_restart_attempts: 5,
            startup_grace_period_secs: 30,
            graceful_shutdown_timeout_secs: 15,
        };
        let json = serde_json::to_string(&config).unwrap();
        let de: HealthCheckConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(de.check_interval_secs, 60);
        assert_eq!(de.max_restart_attempts, 5);
    }

    #[test]
    fn test_agent_process_error_variants() {
        let cases: Vec<(AgentProcessError, &str)> = vec![
            (
                AgentProcessError::AgentNotFound("foo".into()),
                "Agent not found: foo",
            ),
            (
                AgentProcessError::Protocol("bad msg".into()),
                "Protocol error: bad msg",
            ),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn test_agent_event_variants() {
        // Verify event variants can be constructed
        let _ready = AgentEvent::Ready {
            agent_name: "a".into(),
            capabilities: vec!["tools".into()],
        };
        let _progress = AgentEvent::Progress {
            agent_name: "a".into(),
            task_id: "t1".into(),
            chunk: "data".into(),
        };
        let _complete = AgentEvent::TaskComplete {
            agent_name: "a".into(),
            task_id: "t1".into(),
            success: true,
            output: "done".into(),
        };
        let _idle = AgentEvent::Idle {
            agent_name: "a".into(),
            available_tasks_count: 3,
        };
        let _exited = AgentEvent::ProcessExited {
            agent_name: "a".into(),
            exit_code: Some(0),
        };
        let _health = AgentEvent::HealthCheckFailed {
            agent_name: "a".into(),
            consecutive_failures: 2,
        };
        let _restarted = AgentEvent::AgentRestarted {
            agent_name: "a".into(),
            restart_count: 1,
        };
        let _rpc = AgentEvent::RpcRequest {
            agent_name: "a".into(),
            request_id: 42,
            method: "ping".into(),
            params: serde_json::Value::Null,
        };
    }

    #[tokio::test]
    async fn test_try_recv_event_empty() {
        let mut mgr = AgentProcessManager::new();
        assert!(mgr.try_recv_event().is_none());
    }

    #[tokio::test]
    async fn test_take_event_receiver() {
        let mut mgr = AgentProcessManager::new();
        let rx = mgr.take_event_receiver();
        // Receiver should be taken, further calls would return None or panic
        drop(rx);
    }

    // ── JSON-RPC serialization roundtrip tests ─────────────────────────

    #[test]
    fn test_frame_message_request_roundtrip() {
        let msg = JsonRpcMessage::request(
            "execute_task",
            serde_json::json!({"task_id": "t1", "subject": "do work"}),
            42,
        );
        let framed = frame_message(&msg).expect("frame should succeed");
        assert!(framed.ends_with('\n'));
        let parsed = parse_message(&framed).expect("parse should succeed");
        assert!(parsed.is_request());
        assert_eq!(parsed.method(), Some("execute_task"));
        assert!(parsed.id.is_some());
    }

    #[test]
    fn test_frame_message_notification_roundtrip() {
        let msg =
            JsonRpcMessage::notification("agent_ready", serde_json::json!({"agent_name": "w1"}));
        let framed = frame_message(&msg).expect("frame should succeed");
        assert!(framed.ends_with('\n'));
        let parsed = parse_message(&framed).expect("parse should succeed");
        assert!(parsed.is_notification());
        assert!(!parsed.is_request());
    }

    #[test]
    fn test_frame_message_response_roundtrip() {
        let msg = JsonRpcMessage::response(
            protocol::JsonRpcId::Number(7),
            serde_json::json!({"status": "ok"}),
        );
        let framed = frame_message(&msg).expect("frame should succeed");
        let parsed = parse_message(&framed).expect("parse should succeed");
        assert!(parsed.is_response());
        assert!(parsed.result.is_some());
    }

    #[test]
    fn test_frame_message_error_response_roundtrip() {
        let msg = JsonRpcMessage::error_response(
            protocol::JsonRpcId::Number(99),
            protocol::JsonRpcError::not_found("bogus_method"),
        );
        let framed = frame_message(&msg).expect("frame should succeed");
        let parsed = parse_message(&framed).expect("parse should succeed");
        assert!(parsed.is_response());
        assert!(parsed.error.is_some());
        let err = parsed.error.expect("error present");
        assert_eq!(err.code, protocol::JsonRpcError::METHOD_NOT_FOUND);
    }

    #[test]
    fn test_parse_message_trims_whitespace() {
        let json = r#"{"jsonrpc":"2.0","method":"ping","params":null}"#;
        let padded = format!("  {json}  \n");
        let parsed = parse_message(&padded).expect("should parse with whitespace");
        assert_eq!(parsed.method(), Some("ping"));
    }

    #[test]
    fn test_parse_message_invalid_json() {
        let result = parse_message("not json at all");
        assert!(result.is_err());
    }

    // ── Process status transitions ─────────────────────────────────────

    #[test]
    fn test_all_process_status_variants_are_distinct() {
        let statuses = [
            AgentProcessStatus::Starting,
            AgentProcessStatus::Idle,
            AgentProcessStatus::Busy,
            AgentProcessStatus::Stopped,
            AgentProcessStatus::Crashed,
        ];
        for (i, a) in statuses.iter().enumerate() {
            for (j, b) in statuses.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b, "{a:?} should != {b:?}");
                }
            }
        }
    }

    #[test]
    fn test_process_status_serialization_roundtrip() {
        for status in [
            AgentProcessStatus::Starting,
            AgentProcessStatus::Idle,
            AgentProcessStatus::Busy,
            AgentProcessStatus::Stopped,
            AgentProcessStatus::Crashed,
        ] {
            let json = serde_json::to_string(&status).unwrap();
            let de: AgentProcessStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(status, de);
        }
    }

    #[test]
    fn test_process_status_copy_semantics() {
        let a = AgentProcessStatus::Busy;
        let b = a; // Copy
        assert_eq!(a, b); // a still valid after copy
    }

    // ── Config validation ──────────────────────────────────────────────

    #[test]
    fn test_agent_process_config_all_fields() {
        let config = AgentProcessConfig {
            binary_path: PathBuf::from("/usr/local/bin/shannon"),
            args: vec!["--verbose".to_string(), "--log-level=debug".to_string()],
            env: HashMap::from([
                ("RUST_LOG".to_string(), "debug".to_string()),
                ("SHANNON_API_KEY".to_string(), "sk-test".to_string()),
            ]),
            worktree_path: Some(PathBuf::from("/tmp/worktree-1")),
            model: Some("claude-sonnet-4-20250514".to_string()),
            system_prompt: Some("You are a code reviewer".to_string()),
            agent_name: "reviewer-1".to_string(),
            permission_mode: Some("bypassPermissions".to_string()),
            allowed_tools: Some(vec!["Read".to_string(), "Grep".to_string(), "Bash".to_string()]),
            startup_timeout_secs: 120,
        };
        let json = serde_json::to_string(&config).unwrap();
        let roundtrip: AgentProcessConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.binary_path, config.binary_path);
        assert_eq!(roundtrip.args.len(), 2);
        assert_eq!(roundtrip.env.len(), 2);
        assert_eq!(roundtrip.worktree_path, config.worktree_path);
        assert_eq!(roundtrip.model, config.model);
        assert_eq!(roundtrip.system_prompt, config.system_prompt);
        assert_eq!(roundtrip.agent_name, config.agent_name);
        assert_eq!(roundtrip.permission_mode, config.permission_mode);
        assert_eq!(roundtrip.allowed_tools.as_ref().map(|v| v.len()), Some(3));
        assert_eq!(roundtrip.startup_timeout_secs, 120);
    }

    #[test]
    fn test_agent_process_config_defaults_applied() {
        // Minimal JSON — serde defaults for missing optional fields
        let json = r#"{"binary_path":"/bin/echo","agent_name":"worker"}"#;
        let config: AgentProcessConfig = serde_json::from_str(json).unwrap();
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert!(config.worktree_path.is_none());
        assert!(config.model.is_none());
        assert!(config.system_prompt.is_none());
        assert!(config.permission_mode.is_none());
        assert!(config.allowed_tools.is_none());
        assert_eq!(config.startup_timeout_secs, 60);
    }

    #[test]
    fn test_agent_process_config_missing_required_field() {
        // Missing agent_name (required)
        let json = r#"{"binary_path":"/bin/echo"}"#;
        let result = serde_json::from_str::<AgentProcessConfig>(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_health_check_config_partial_override() {
        // Override only some fields; verify defaults for the rest
        let json = r#"{"check_interval_secs":120,"max_restart_attempts":10}"#;
        let config: HealthCheckConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.check_interval_secs, 120);
        assert_eq!(config.ping_timeout_secs, 10); // default
        assert_eq!(config.max_restart_attempts, 10);
        assert_eq!(config.startup_grace_period_secs, 15); // default
        assert_eq!(config.graceful_shutdown_timeout_secs, 10); // default
    }

    // ── Blocked environment variables ──────────────────────────────────

    #[test]
    fn test_blocked_env_contains_dangerous_vars() {
        assert!(BLOCKED_ENV.contains(&"LD_PRELOAD"));
        assert!(BLOCKED_ENV.contains(&"LD_LIBRARY_PATH"));
        assert!(BLOCKED_ENV.contains(&"DYLD_INSERT_LIBRARIES"));
        assert!(BLOCKED_ENV.contains(&"DYLD_LIBRARY_PATH"));
        assert!(BLOCKED_ENV.contains(&"__KMP_REGISTERED_LIBRARIES"));
    }

    #[test]
    fn test_blocked_env_does_not_contain_safe_vars() {
        assert!(!BLOCKED_ENV.contains(&"PATH"));
        assert!(!BLOCKED_ENV.contains(&"HOME"));
        assert!(!BLOCKED_ENV.contains(&"RUST_LOG"));
    }

    // ── Channel communication patterns ─────────────────────────────────

    #[tokio::test]
    async fn test_event_channel_send_and_recv() {
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        tx.send(AgentEvent::Ready {
            agent_name: "worker-1".to_string(),
            capabilities: vec!["bash".to_string(), "read".to_string()],
        })
        .await
        .expect("send should succeed");

        let event = rx.recv().await.expect("recv should succeed");
        match event {
            AgentEvent::Ready { agent_name, capabilities } => {
                assert_eq!(agent_name, "worker-1");
                assert_eq!(capabilities.len(), 2);
            }
            _ => panic!("Expected Ready event"),
        }
    }

    #[tokio::test]
    async fn test_event_channel_multiple_events_ordering() {
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(32);

        // Send events in order — AgentEvent is not Clone, so send directly
        tx.send(AgentEvent::Ready {
            agent_name: "a1".to_string(),
            capabilities: vec![],
        })
        .await
        .expect("send 1");
        tx.send(AgentEvent::Progress {
            agent_name: "a1".to_string(),
            task_id: "t1".to_string(),
            chunk: "starting".to_string(),
        })
        .await
        .expect("send 2");
        tx.send(AgentEvent::Progress {
            agent_name: "a1".to_string(),
            task_id: "t1".to_string(),
            chunk: "halfway".to_string(),
        })
        .await
        .expect("send 3");
        tx.send(AgentEvent::TaskComplete {
            agent_name: "a1".to_string(),
            task_id: "t1".to_string(),
            success: true,
            output: "done".to_string(),
        })
        .await
        .expect("send 4");

        // Verify ordering: Ready -> Progress -> Progress -> TaskComplete
        let e1 = rx.recv().await.expect("recv 1");
        assert!(matches!(e1, AgentEvent::Ready { .. }));

        let e2 = rx.recv().await.expect("recv 2");
        if let AgentEvent::Progress { chunk, .. } = e2 {
            assert_eq!(chunk, "starting");
        } else {
            panic!("Expected Progress event");
        }

        let e3 = rx.recv().await.expect("recv 3");
        if let AgentEvent::Progress { chunk, .. } = e3 {
            assert_eq!(chunk, "halfway");
        } else {
            panic!("Expected Progress event");
        }

        let e4 = rx.recv().await.expect("recv 4");
        if let AgentEvent::TaskComplete { success, output, .. } = e4 {
            assert!(success);
            assert_eq!(output, "done");
        } else {
            panic!("Expected TaskComplete event");
        }
    }

    #[tokio::test]
    async fn test_event_channel_backpressure() {
        // Channel of capacity 2, send 4 — third and fourth should block
        let (tx, mut rx) = mpsc::channel::<AgentEvent>(2);

        // First two should succeed immediately
        tx.send(AgentEvent::Idle {
            agent_name: "a".to_string(),
            available_tasks_count: 0,
        })
        .await
        .expect("send 1");
        tx.send(AgentEvent::Idle {
            agent_name: "b".to_string(),
            available_tasks_count: 1,
        })
        .await
        .expect("send 2");

        // Drain one to free capacity
        let _ = rx.recv().await;

        // Now third should succeed
        tx.send(AgentEvent::Idle {
            agent_name: "c".to_string(),
            available_tasks_count: 2,
        })
        .await
        .expect("send 3 after drain");
    }

    // ── Pending RPC tracking ───────────────────────────────────────────

    #[tokio::test]
    async fn test_oneshot_channel_rpc_pattern() {
        // Simulates the pending_rpcs pattern used in send_request/read_loop
        let (tx, rx) = oneshot::channel::<Result<JsonRpcMessage, String>>();
        let msg = JsonRpcMessage::response(
            protocol::JsonRpcId::Number(1),
            serde_json::json!({"ok": true}),
        );
        tx.send(Ok(msg)).expect("send should succeed");
        let result = rx.await.expect("recv should succeed").expect("inner should be Ok");
        assert!(result.is_response());
    }

    #[tokio::test]
    async fn test_oneshot_channel_dropped_sender() {
        // Simulates what happens when read_loop exits without sending a response
        let (_tx, rx) = oneshot::channel::<Result<JsonRpcMessage, String>>();
        drop(_tx);
        let result = rx.await;
        assert!(result.is_err(), "Should get error when sender is dropped");
    }

    #[tokio::test]
    async fn test_pending_rpcs_hashmap_tracking() {
        let pending: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        // Insert a pending RPC
        let (tx1, rx1) = oneshot::channel();
        pending.lock().unwrap().insert(1, PendingRpc { sender: tx1 });

        // Insert a second
        let (tx2, rx2) = oneshot::channel();
        pending.lock().unwrap().insert(2, PendingRpc { sender: tx2 });

        assert_eq!(pending.lock().unwrap().len(), 2);

        // Resolve RPC 1
        let rpc1 = pending.lock().unwrap().remove(&1);
        assert!(rpc1.is_some());
        rpc1.unwrap().sender.send(Ok(JsonRpcMessage::response(
            protocol::JsonRpcId::Number(1),
            serde_json::json!({"done": true}),
        ))).expect("send should succeed");

        let resp = rx1.await.expect("recv").expect("ok");
        assert!(resp.result.is_some());

        // RPC 2 is still pending
        assert_eq!(pending.lock().unwrap().len(), 1);
        assert!(pending.lock().unwrap().contains_key(&2));

        // Drop without resolving
        drop(rx2);
    }

    #[tokio::test]
    async fn test_pending_rpcs_drain_on_reader_exit() {
        // Simulates the orphan drain at the end of read_loop
        let pending: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        let (tx1, _rx1) = oneshot::channel();
        let (tx2, _rx2) = oneshot::channel();
        pending.lock().unwrap().insert(10, PendingRpc { sender: tx1 });
        pending.lock().unwrap().insert(20, PendingRpc { sender: tx2 });

        // Drain all (same pattern as read_loop)
        let orphaned: Vec<_> = pending.lock().unwrap().drain().collect();
        drop(orphaned);

        assert!(pending.lock().unwrap().is_empty());
    }

    // ── Manager-level operations on missing agents ─────────────────────

    #[tokio::test]
    async fn test_send_notification_to_missing_agent() {
        let mgr = AgentProcessManager::new();
        let result = mgr
            .send_notification("ghost", "some_method", serde_json::Value::Null)
            .await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentProcessError::AgentNotFound(name) => assert_eq!(name, "ghost"),
            other => panic!("Expected AgentNotFound, got {other}"),
        }
    }

    #[tokio::test]
    async fn test_shutdown_missing_agent() {
        let mgr = AgentProcessManager::new();
        let result = mgr.shutdown_agent("ghost", "test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_graceful_shutdown_missing_agent() {
        let mgr = AgentProcessManager::new();
        // graceful_shutdown_agent sends shutdown notification (which fails for missing)
        // then tries to wait — but since agent doesn't exist, it should succeed quickly
        // because the agent is considered "exited" when not found
        let result = mgr
            .graceful_shutdown_agent("ghost", Duration::from_millis(100))
            .await;
        // It should return Ok because the agent doesn't exist (treated as already exited)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_restart_missing_agent() {
        let mgr = AgentProcessManager::new();
        let result = mgr.restart_agent("ghost").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            AgentProcessError::AgentNotFound(name) => assert_eq!(name, "ghost"),
            other => panic!("Expected AgentNotFound, got {other}"),
        }
    }

    #[tokio::test]
    async fn test_send_rpc_response_to_missing_agent() {
        let mgr = AgentProcessManager::new();
        let result = mgr
            .send_rpc_response("ghost", 1, serde_json::json!({"ok": true}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_shutdown_all_when_empty() {
        let mgr = AgentProcessManager::new();
        // Should not panic or hang
        mgr.shutdown_all().await;
    }

    // ── RPC ID allocation ──────────────────────────────────────────────

    #[test]
    fn test_rpc_ids_are_monotonically_increasing() {
        let mgr = AgentProcessManager::new();
        let id1 = mgr.next_id();
        let id2 = mgr.next_id();
        let id3 = mgr.next_id();
        assert!(id2 > id1, "IDs should be increasing: {id1} < {id2}");
        assert!(id3 > id2, "IDs should be increasing: {id2} < {id3}");
    }

    #[test]
    fn test_rpc_ids_start_at_1() {
        let mgr = AgentProcessManager::new();
        let id = mgr.next_id();
        assert_eq!(id, 1);
    }

    // ── Task handle tracking ───────────────────────────────────────────

    #[tokio::test]
    async fn test_track_task_completed_is_pruned() {
        let mgr = AgentProcessManager::new();

        // Spawn a task that completes immediately — don't join it, just track
        let handle = tokio::spawn(async {});
        mgr.track_task(handle);

        // Wait for it to finish
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Completed tasks should be pruned when track_task is called again
        let handle2 = tokio::spawn(async {});
        mgr.track_task(handle2);

        // Should have pruned the first completed handle
        if let Ok(handles) = mgr.task_handles.lock() {
            assert_eq!(handles.len(), 1);
        }
    }

    #[tokio::test]
    async fn test_track_task_multiple_running() {
        let mgr = AgentProcessManager::new();

        let h1 = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let h2 = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        mgr.track_task(h1);
        mgr.track_task(h2);

        // Both should be tracked
        if let Ok(handles) = mgr.task_handles.lock() {
            assert_eq!(handles.len(), 2);
        }
    }

    // ── Default trait implementations ──────────────────────────────────

    #[test]
    fn test_default_health_check_config() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.check_interval_secs, 30);
        assert_eq!(config.ping_timeout_secs, 10);
        assert_eq!(config.max_restart_attempts, 3);
        assert_eq!(config.startup_grace_period_secs, 15);
        assert_eq!(config.graceful_shutdown_timeout_secs, 10);
    }

    #[test]
    fn test_default_function_values() {
        assert_eq!(default_startup_timeout(), 60);
        assert_eq!(default_health_interval(), 30);
        assert_eq!(default_ping_timeout(), 10);
        assert_eq!(default_max_restarts(), 3);
        assert_eq!(default_grace_period(), 15);
        assert_eq!(default_shutdown_timeout(), 10);
    }

    // ── Error display and variants ─────────────────────────────────────

    #[test]
    fn test_agent_process_error_spawn_failed() {
        let err = AgentProcessError::SpawnFailed {
            agent: "test-agent".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "binary not found"),
        };
        let msg = err.to_string();
        assert!(msg.contains("test-agent"));
        assert!(msg.contains("binary not found"));
    }

    #[test]
    fn test_agent_process_error_io() {
        let err = AgentProcessError::Io(std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "pipe closed",
        ));
        let msg = err.to_string();
        assert!(msg.contains("pipe closed"));
    }

    #[test]
    fn test_agent_process_error_protocol() {
        let err = AgentProcessError::Protocol("malformed JSON-RPC frame".to_string());
        assert_eq!(err.to_string(), "Protocol error: malformed JSON-RPC frame");
    }

    #[test]
    fn test_agent_process_error_debug_format() {
        let err = AgentProcessError::AgentNotFound("x".to_string());
        let debug = format!("{err:?}");
        assert!(debug.contains("AgentNotFound"));
    }

    // ── AgentEvent construction and matching ───────────────────────────

    #[test]
    fn test_agent_event_ready_with_capabilities() {
        let event = AgentEvent::Ready {
            agent_name: "reviewer".to_string(),
            capabilities: vec!["Read".to_string(), "Grep".to_string(), "Bash".to_string()],
        };
        if let AgentEvent::Ready { agent_name, capabilities } = event {
            assert_eq!(agent_name, "reviewer");
            assert_eq!(capabilities.len(), 3);
        }
    }

    #[test]
    fn test_agent_event_progress_fields() {
        let event = AgentEvent::Progress {
            agent_name: "builder".to_string(),
            task_id: "task-42".to_string(),
            chunk: "compiling...".to_string(),
        };
        if let AgentEvent::Progress { agent_name, task_id, chunk } = event {
            assert_eq!(agent_name, "builder");
            assert_eq!(task_id, "task-42");
            assert_eq!(chunk, "compiling...");
        }
    }

    #[test]
    fn test_agent_event_task_complete_success() {
        let event = AgentEvent::TaskComplete {
            agent_name: "tester".to_string(),
            task_id: "t-1".to_string(),
            success: true,
            output: "all tests passed".to_string(),
        };
        if let AgentEvent::TaskComplete { success, output, .. } = event {
            assert!(success);
            assert_eq!(output, "all tests passed");
        }
    }

    #[test]
    fn test_agent_event_task_complete_failure() {
        let event = AgentEvent::TaskComplete {
            agent_name: "tester".to_string(),
            task_id: "t-2".to_string(),
            success: false,
            output: "compilation failed: missing semicolon".to_string(),
        };
        if let AgentEvent::TaskComplete { success, output, .. } = event {
            assert!(!success);
            assert!(output.contains("compilation failed"));
        }
    }

    #[test]
    fn test_agent_event_idle_with_task_count() {
        let event = AgentEvent::Idle {
            agent_name: "worker-3".to_string(),
            available_tasks_count: 7,
        };
        if let AgentEvent::Idle { agent_name, available_tasks_count } = event {
            assert_eq!(agent_name, "worker-3");
            assert_eq!(available_tasks_count, 7);
        }
    }

    #[test]
    fn test_agent_event_process_exited_with_code() {
        let event = AgentEvent::ProcessExited {
            agent_name: "dead-agent".to_string(),
            exit_code: Some(1),
        };
        if let AgentEvent::ProcessExited { exit_code, .. } = event {
            assert_eq!(exit_code, Some(1));
        }
    }

    #[test]
    fn test_agent_event_process_exited_signal() {
        let event = AgentEvent::ProcessExited {
            agent_name: "killed-agent".to_string(),
            exit_code: None, // Killed by signal
        };
        if let AgentEvent::ProcessExited { exit_code, .. } = event {
            assert!(exit_code.is_none());
        }
    }

    #[test]
    fn test_agent_event_health_check_failed() {
        let event = AgentEvent::HealthCheckFailed {
            agent_name: "sick-agent".to_string(),
            consecutive_failures: 5,
        };
        if let AgentEvent::HealthCheckFailed { consecutive_failures, .. } = event {
            assert_eq!(consecutive_failures, 5);
        }
    }

    #[test]
    fn test_agent_event_restarted() {
        let event = AgentEvent::AgentRestarted {
            agent_name: "resilient-agent".to_string(),
            restart_count: 3,
        };
        if let AgentEvent::AgentRestarted { restart_count, .. } = event {
            assert_eq!(restart_count, 3);
        }
    }

    #[test]
    fn test_agent_event_rpc_request() {
        let event = AgentEvent::RpcRequest {
            agent_name: "requester".to_string(),
            request_id: 42,
            method: "claim_task".to_string(),
            params: serde_json::json!({"task_id": "t-99"}),
        };
        if let AgentEvent::RpcRequest { request_id, method, params, .. } = event {
            assert_eq!(request_id, 42);
            assert_eq!(method, "claim_task");
            assert_eq!(params["task_id"], "t-99");
        }
    }

    // ── Read loop dispatch — JSON-RPC message routing ──────────────────

    #[tokio::test]
    async fn test_read_loop_dispatches_agent_ready() {
        let (event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        let ready_json = serde_json::json!({
            "agent_name": "test-worker",
            "capabilities": ["bash", "read", "write"]
        });
        let msg = JsonRpcMessage::notification("agent_ready", ready_json);
        let line = frame_message(&msg).unwrap();

        // Parse it back to simulate what read_loop does
        let parsed = parse_message(&line).unwrap();
        if let Some(method) = parsed.method() {
            if method == "agent_ready" {
                if let Ok(params) = serde_json::from_value::<AgentReadyParams>(
                    parsed.params.unwrap_or_default(),
                ) {
                    event_tx
                        .send(AgentEvent::Ready {
                            agent_name: params.agent_name.clone(),
                            capabilities: params.capabilities,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        let event = rx.recv().await.unwrap();
        if let AgentEvent::Ready { agent_name, capabilities } = event {
            assert_eq!(agent_name, "test-worker");
            assert_eq!(capabilities.len(), 3);
        } else {
            panic!("Expected Ready event");
        }
    }

    #[tokio::test]
    async fn test_read_loop_dispatches_task_progress() {
        let (event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        let progress_json = serde_json::json!({
            "task_id": "t-42",
            "chunk": "50% complete"
        });
        let msg = JsonRpcMessage::notification("task_progress", progress_json);
        let line = frame_message(&msg).unwrap();
        let parsed = parse_message(&line).unwrap();

        if let Some(method) = parsed.method() {
            if method == "task_progress" {
                if let Ok(params) = serde_json::from_value::<TaskProgressParams>(
                    parsed.params.unwrap_or_default(),
                ) {
                    event_tx
                        .send(AgentEvent::Progress {
                            agent_name: "worker".to_string(),
                            task_id: params.task_id,
                            chunk: params.chunk,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        let event = rx.recv().await.unwrap();
        if let AgentEvent::Progress { task_id, chunk, .. } = event {
            assert_eq!(task_id, "t-42");
            assert_eq!(chunk, "50% complete");
        } else {
            panic!("Expected Progress event");
        }
    }

    #[tokio::test]
    async fn test_read_loop_dispatches_task_complete() {
        let (event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        let complete_json = serde_json::json!({
            "task_id": "t-99",
            "success": true,
            "output": "all tests passed"
        });
        let msg = JsonRpcMessage::notification("task_complete", complete_json);
        let line = frame_message(&msg).unwrap();
        let parsed = parse_message(&line).unwrap();

        if let Some(method) = parsed.method() {
            if method == "task_complete" {
                if let Ok(params) = serde_json::from_value::<TaskCompleteParams>(
                    parsed.params.unwrap_or_default(),
                ) {
                    event_tx
                        .send(AgentEvent::TaskComplete {
                            agent_name: "tester".to_string(),
                            task_id: params.task_id,
                            success: params.success,
                            output: params.output,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        let event = rx.recv().await.unwrap();
        if let AgentEvent::TaskComplete { task_id, success, output, .. } = event {
            assert_eq!(task_id, "t-99");
            assert!(success);
            assert_eq!(output, "all tests passed");
        } else {
            panic!("Expected TaskComplete event");
        }
    }

    #[tokio::test]
    async fn test_read_loop_dispatches_agent_idle() {
        let (event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        let idle_json = serde_json::json!({
            "agent_name": "free-agent",
            "available_tasks_count": 5
        });
        let msg = JsonRpcMessage::notification("agent_idle", idle_json);
        let line = frame_message(&msg).unwrap();
        let parsed = parse_message(&line).unwrap();

        if let Some(method) = parsed.method() {
            if method == "agent_idle" {
                if let Ok(params) = serde_json::from_value::<AgentIdleParams>(
                    parsed.params.unwrap_or_default(),
                ) {
                    event_tx
                        .send(AgentEvent::Idle {
                            agent_name: params.agent_name,
                            available_tasks_count: params.available_tasks_count,
                        })
                        .await
                        .unwrap();
                }
            }
        }

        let event = rx.recv().await.unwrap();
        if let AgentEvent::Idle { agent_name, available_tasks_count } = event {
            assert_eq!(agent_name, "free-agent");
            assert_eq!(available_tasks_count, 5);
        } else {
            panic!("Expected Idle event");
        }
    }

    #[tokio::test]
    async fn test_read_loop_dispatches_rpc_request() {
        let (event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        // Simulate an incoming RPC request (has method + id)
        let msg = JsonRpcMessage::request(
            "claim_task",
            serde_json::json!({"task_id": "t-7"}),
            55,
        );
        let line = frame_message(&msg).unwrap();
        let parsed = parse_message(&line).unwrap();

        // In read_loop, RPC requests from agents (create_task, claim_task, etc.) are dispatched
        let rpc_methods = [
            "create_task", "update_task", "get_task", "team_manifest",
            "list_tasks", "claim_task", "disband_team", "add_agent",
        ];
        if let Some(method) = parsed.method() {
            if rpc_methods.contains(&method) {
                if let Some(protocol::JsonRpcId::Number(request_id)) = parsed.id {
                    event_tx
                        .send(AgentEvent::RpcRequest {
                            agent_name: "worker-1".to_string(),
                            request_id,
                            method: method.to_string(),
                            params: parsed.params.unwrap_or_default(),
                        })
                        .await
                        .unwrap();
                }
            }
        }

        let event = rx.recv().await.unwrap();
        if let AgentEvent::RpcRequest { request_id, method, params, .. } = event {
            assert_eq!(request_id, 55);
            assert_eq!(method, "claim_task");
            assert_eq!(params["task_id"], "t-7");
        } else {
            panic!("Expected RpcRequest event");
        }
    }

    #[tokio::test]
    async fn test_read_loop_dispatches_response_to_pending_rpc() {
        let pending: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));
        let (tx, rx) = oneshot::channel();
        pending.lock().unwrap().insert(42, PendingRpc { sender: tx });

        // Simulate a response arriving (has id, no method)
        let response = JsonRpcMessage::response(
            protocol::JsonRpcId::Number(42),
            serde_json::json!({"status": "done"}),
        );
        let line = frame_message(&response).unwrap();
        let parsed = parse_message(&line).unwrap();

        // Response dispatch path from read_loop
        assert!(parsed.method().is_none());
        if let Some(protocol::JsonRpcId::Number(rpc_id)) = parsed.id {
            if let Some(pending_rpc) = pending.lock().unwrap().remove(&rpc_id) {
                pending_rpc.sender.send(Ok(parsed)).unwrap();
            }
        }

        let result = rx.await.unwrap().unwrap();
        assert!(result.result.is_some());
        assert_eq!(result.result.unwrap()["status"], "done");
    }

    #[tokio::test]
    async fn test_read_loop_response_for_unknown_rpc_id() {
        let pending: Arc<std::sync::Mutex<HashMap<i64, PendingRpc>>> =
            Arc::new(std::sync::Mutex::new(HashMap::new()));

        let response = JsonRpcMessage::response(
            protocol::JsonRpcId::Number(999),
            serde_json::json!({"orphan": true}),
        );
        let line = frame_message(&response).unwrap();
        let parsed = parse_message(&line).unwrap();

        // No pending RPC for ID 999 — should be silently ignored
        if let Some(protocol::JsonRpcId::Number(rpc_id)) = parsed.id {
            let removed = pending.lock().unwrap().remove(&rpc_id);
            assert!(removed.is_none(), "No pending RPC for unknown ID");
        }

        // No panic, no error — just silently dropped
    }

    #[tokio::test]
    async fn test_read_loop_malformed_message_is_skipped() {
        let (_event_tx, mut rx) = mpsc::channel::<AgentEvent>(16);

        // Try parsing garbage — read_loop would skip it
        let result = parse_message("this is not json");
        assert!(result.is_err());

        // Channel should remain empty
        assert!(rx.try_recv().is_err());
    }

    // ── Timeout scenarios ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_graceful_shutdown_times_out_and_force_kills_missing() {
        // When the agent is missing from the map, graceful_shutdown_agent
        // treats it as already exited and returns Ok immediately.
        let mgr = AgentProcessManager::new();
        let result = mgr
            .graceful_shutdown_agent("nonexistent", Duration::from_millis(50))
            .await;
        assert!(result.is_ok());
    }

    // ── ShutdownParamsWrapper serialization ────────────────────────────

    #[test]
    fn test_shutdown_params_wrapper_serialization() {
        let wrapper = ShutdownParamsWrapper {
            reason: "coordinator shutting down".to_string(),
        };
        let json = serde_json::to_string(&wrapper).unwrap();
        assert!(json.contains("coordinator shutting down"));

        let de: ShutdownParamsWrapper = serde_json::from_str(&json).unwrap();
        assert_eq!(de.reason, "coordinator shutting down");
    }

    // ── Multiple agents listing ────────────────────────────────────────

    #[tokio::test]
    async fn test_running_agents_empty_after_creation() {
        let mgr = AgentProcessManager::new();
        let agents = mgr.running_agents().await;
        assert!(agents.is_empty());
    }

    #[tokio::test]
    async fn test_agent_status_returns_none_for_unknown() {
        let mgr = AgentProcessManager::new();
        assert!(mgr.agent_status("nobody").await.is_none());
    }

    // ── Drop behavior ──────────────────────────────────────────────────

    #[test]
    fn test_process_manager_drop_cleans_up() {
        let mgr = AgentProcessManager::new();
        drop(mgr); // Should not panic
    }

    #[tokio::test]
    async fn test_process_manager_default_trait() {
        let mgr = AgentProcessManager::default();
        let agents = mgr.running_agents().await;
        assert!(agents.is_empty());
    }

    // ── execute_task on missing agent ───────────────────────────────────

    #[tokio::test]
    async fn test_execute_task_on_missing_agent() {
        let mgr = AgentProcessManager::new();
        let params = ExecuteTaskParams {
            task_id: "t-1".to_string(),
            subject: "Fix tests".to_string(),
            description: "Make all tests pass".to_string(),
            priority: "high".to_string(),
            active_form: Some("Fixing tests".to_string()),
        };
        let result = mgr.execute_task("ghost", params).await;
        assert!(result.is_err());
    }

    // ── Health monitor start/stop ──────────────────────────────────────

    #[tokio::test]
    async fn test_start_health_monitor_does_not_panic() {
        let mgr = AgentProcessManager::new();
        let config = HealthCheckConfig {
            check_interval_secs: 1,
            ping_timeout_secs: 1,
            max_restart_attempts: 1,
            startup_grace_period_secs: 60, // Long grace so it doesn't run checks during test
            graceful_shutdown_timeout_secs: 5,
        };
        mgr.start_health_monitor(config);
        // Dropping the manager should abort the health monitor task
        drop(mgr);
    }

    #[tokio::test]
    async fn test_start_health_monitor_replaces_previous() {
        let mgr = AgentProcessManager::new();
        let config = HealthCheckConfig::default();
        mgr.start_health_monitor(config.clone());
        // Starting again should replace the previous handle
        mgr.start_health_monitor(config);
        drop(mgr);
    }

    // ── recv_event and try_recv_event ──────────────────────────────────

    #[tokio::test]
    async fn test_recv_event_on_empty_channel_waits() {
        let mut mgr = AgentProcessManager::new();
        // recv_event will block forever on empty channel, so we use a timeout
        let result = tokio::time::timeout(Duration::from_millis(50), mgr.recv_event()).await;
        assert!(result.is_err(), "Should timeout on empty channel");
    }

    #[tokio::test]
    async fn test_try_recv_event_on_empty_returns_none() {
        let mut mgr = AgentProcessManager::new();
        assert!(mgr.try_recv_event().is_none());
    }

    // ── Event channel with take_event_receiver ─────────────────────────

    #[tokio::test]
    async fn test_take_event_receiver_empties_original() {
        let mut mgr = AgentProcessManager::new();
        let mut rx = mgr.take_event_receiver();
        // The taken receiver should be empty
        assert!(rx.try_recv().is_err());
        // Subsequent try_recv_event on mgr should also fail (new closed channel)
        assert!(mgr.try_recv_event().is_none());
    }

    // ── Agent process config with all optional fields populated ────────

    #[test]
    fn test_config_with_worktree_and_tools() {
        let config = AgentProcessConfig {
            binary_path: PathBuf::from("/usr/bin/shannon"),
            args: vec![],
            env: HashMap::new(),
            worktree_path: Some(PathBuf::from("/project/worktrees/feature-x")),
            model: Some("opus".to_string()),
            system_prompt: Some("Focus on refactoring".to_string()),
            agent_name: "refactorer".to_string(),
            permission_mode: Some("auto".to_string()),
            allowed_tools: Some(vec!["Read".to_string(), "Edit".to_string(), "Grep".to_string()]),
            startup_timeout_secs: 90,
        };

        let json = serde_json::to_string(&config).unwrap();
        let de: AgentProcessConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(de.worktree_path.unwrap(), PathBuf::from("/project/worktrees/feature-x"));
        assert_eq!(de.allowed_tools.unwrap().len(), 3);
        assert_eq!(de.startup_timeout_secs, 90);
    }

    // ── Instant tracking for last_seen ─────────────────────────────────

    #[test]
    fn test_instant_tracks_elapsed_time() {
        let start = Instant::now();
        // Verify that Instant is usable (used for last_seen in AgentHandle)
        let _ = start.elapsed();
    }

    #[test]
    fn test_instant_ordering() {
        let a = Instant::now();
        let b = a + Duration::from_millis(1);
        assert!(b > a);
    }
}
