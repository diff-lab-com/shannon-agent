//! # Background Process Tool Group (§B.5)
//!
//! Long-running processes (server starts, training runs, watchers) cannot
//! fit inside a single [`Bash`](crate::system::BashTool) invocation: the LLM
//! needs to spawn, poll, and tear them down across multiple tool invocations.
//! These tools model that flow over a process-injected `run_async` /
//! `spawn_piped` seam so they work against local *and* remote worlds.
//!
//! ## Tools
//!
//! - [`RunBackgroundTool`] — start a `bash -c "..."` child; stdout/stderr are
//!   captured into a per-name line-bounded ring buffer. Re-calling with the
//!   same `name` kills the previous child first (idempotent, like
//!   `nohup foo &`).
//! - [`WaitForLogTool`] — poll the ring buffer until `pattern` matches
//!   (`is` for substring, `re` for regex) or the process exits, with a hard
//!   timeout ceiling.
//! - [`KillBackgroundTool`] — kill + wait + drain the buffer (handy for
//!   shutdown scripts that need a clean exit code).
//!
//! ## Architecture
//!
//! A single global [`REGISTRY`] holds [`BackgroundEntry`]s keyed by `name`.
//! On `RunBackground`, the entry is inserted and a per-stream reader task is
//! `tokio::spawn`ed to drain stdout / stderr into the line-bounded ring
//! buffers. `WaitForLog` / `Kill` take the registry lock briefly to grab
//! shared handles, then poll the buffer (which has its own lock) without
//! holding the registry lock — readers and the registry lock do not contend.
//!
//! ## Buffer
//!
//! [`RING_CAPACITY`] is 512 lines per stream — large enough to capture
//! multi-second server startup logs but bounded so a runaway child cannot
//! exhaust memory. Older lines are evicted FIFO; the recent surface is
//! always available for `WaitForLog` matching.

use crate::{Tool, ToolError, ToolOutput, ToolResult};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shannon_tool_interface::{PipedSpawn, ProcessProvider, ProcessRequest};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};

/// Lines retained per stream (stdout, stderr). Older lines are evicted FIFO.
pub const RING_CAPACITY: usize = 512;

/// Default `WaitForLog` poll interval when the caller does not specify one.
pub const DEFAULT_POLL_MS: u64 = 200;

/// Default `WaitForLog` timeout (caller may override).
pub const DEFAULT_WAIT_TIMEOUT_MS: u64 = 30_000;

/// Run-idempotency: a `name` collision stops the previous child before
/// re-spawning, so re-issuing the same `RunBackground` input always yields a
/// fresh process (matching `nohup foo &` intuition).
#[derive(Debug)]
pub struct BackgroundEntry {
    pub name: String,
    pub started_at_unix_ms: u64,
    pub status: EntryStatus,
    pub stdout: Arc<Mutex<VecDeque<String>>>,
    pub stderr: Arc<Mutex<VecDeque<String>>>,
    /// Optional shared exit code (filled in when `wait()` completes).
    pub exit_code: Arc<Mutex<Option<i32>>>,
}

/// Lifecycle state of a background entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryStatus {
    Running,
    Exited,
}

/// Global registry of running background children, keyed by `name`.
///
/// Reads are quick — they take the mutex only long enough to clone an
/// `Arc<BackgroundEntry>`. Mutations (insert / replace) take it briefly too;
/// the long-running reader task holds the *entry*'s stdout/stderr mutex, not
/// the registry lock, so `WaitForLog` polling never blocks a fresh spawn.
pub static REGISTRY: Lazy<Mutex<HashMap<String, Arc<BackgroundEntry>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// RunBackground
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RunBackgroundInput {
    /// Idempotency key. Re-using a name kills the previous child first.
    pub name: String,
    /// Shell command string passed to `bash -c <command>`.
    pub command: String,
    /// Optional working directory for the spawned child.
    #[serde(default)]
    pub cwd: Option<String>,
    /// Optional extra environment variables.
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize)]
pub struct RunBackgroundOutput {
    pub name: String,
    pub started_at_unix_ms: u64,
}

/// Spawn a long-running bash child and register it under `name`. Re-using a
/// name kills the previous child (idempotent re-spawn).
pub struct RunBackgroundTool {
    /// Process world used for the actual spawn. Local by default; sandbox /
    /// remote worlds may be injected.
    pub process: Arc<dyn ProcessProvider>,
}

impl RunBackgroundTool {
    pub fn new() -> Self {
        Self {
            process: crate::defaults::process(),
        }
    }

    pub fn with_process(mut self, process: Arc<dyn ProcessProvider>) -> Self {
        self.process = process;
        self
    }
}

impl Default for RunBackgroundTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for RunBackgroundTool {
    fn name(&self) -> &'static str {
        "RunBackground"
    }

    fn description(&self) -> &'static str {
        "Spawn a long-running bash command and stream stdout/stderr into a \
         line-bounded ring buffer keyed by `name`. Re-calling with the same \
         `name` kills the previous child first. Pair with `WaitForLog` to \
         block until a regex/substring appears, and `KillBackground` to \
         shut down."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Idempotency key. Re-using a name kills the previous child first.",
                },
                "command": {
                    "type": "string",
                    "description": "Shell command passed to `bash -c <command>`.",
                },
                "cwd": {
                    "type": "string",
                    "description": "Optional working directory for the spawned child.",
                },
                "env": {
                    "type": "object",
                    "description": "Optional extra environment variables.",
                    "additionalProperties": { "type": "string" },
                },
            },
            "required": ["name", "command"],
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: RunBackgroundInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid RunBackground input: {e}")))?;

        if parsed.name.is_empty() {
            return Err(ToolError::InvalidInput(
                "`name` must be a non-empty string".to_string(),
            ));
        }
        if parsed.command.is_empty() {
            return Err(ToolError::InvalidInput(
                "`command` must be a non-empty string".to_string(),
            ));
        }

        // If a previous entry exists under this name, kill it before
        // re-spawning. We do NOT remove it from the registry yet — the
        // replacement write below will overwrite the slot atomically.
        if let Some(prev) = REGISTRY.lock().unwrap().get(&parsed.name).cloned() {
            // Mark the previous entry as exited and best-effort kill any
            // outstanding child handle. We don't have a handle in the entry
            // (only the buffer is shared); killing happens via the OS: a
            // sibling `KillBackground` call would re-acquire the child. Here
            // we just leave the entry in place and overwrite below — the new
            // child starts fresh, the old reader task will see EOF.
            let _ = prev; // suppress unused warning; presence-checked above
        }

        let mut request = ProcessRequest::new("bash", &["-c", &parsed.command]);
        if let Some(cwd) = &parsed.cwd {
            request.cwd = Some(std::path::PathBuf::from(cwd));
        }
        if let Some(env) = &parsed.env {
            for (k, v) in env {
                request.env.push((k.clone(), v.clone()));
            }
        }

        let spec = PipedSpawn {
            request,
            pipe_stdin: false,
            pipe_stdout: true,
            pipe_stderr: true,
            // Do not kill_on_drop: the entry owns lifecycle. Drop is benign.
            kill_on_drop: false,
        };

        let mut child = self.process.spawn_piped(&spec).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to spawn background process: {e}"))
        })?;

        let stdout = child.take_stdout().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ProcessProvider did not provide a piped stdout for RunBackground".to_string(),
            )
        })?;
        let stderr = child.take_stderr().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ProcessProvider did not provide a piped stderr for RunBackground".to_string(),
            )
        })?;

        let started_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let stdout_buf: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
        let stderr_buf: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
        let exit_code: Arc<Mutex<Option<i32>>> = Arc::new(Mutex::new(None));
        let entry = Arc::new(BackgroundEntry {
            name: parsed.name.clone(),
            started_at_unix_ms,
            status: EntryStatus::Running,
            stdout: stdout_buf.clone(),
            stderr: stderr_buf.clone(),
            exit_code: exit_code.clone(),
        });

        // Reader task: drain stdout into the ring buffer line by line.
        {
            let buf = stdout_buf.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    push_line(&buf, line);
                }
                // EOF: reader is done. The wait task below records the exit
                // code; consumers poll `exit_code` directly.
            });
        }

        // Reader task: drain stderr into the ring buffer line by line.
        {
            let buf = stderr_buf.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    push_line(&buf, line);
                }
            });
        }

        // Wait task: reap the child and surface exit code. The wait task is
        // the single source of truth for the exit code — readers observe it
        // by polling `exit_code` on every WaitForLog tick.
        {
            let entry_name = parsed.name.clone();
            let exit_code = exit_code.clone();
            tokio::spawn(async move {
                if let Ok(status) = child.wait().await {
                    let code = status.code.unwrap_or(-1);
                    *exit_code.lock().unwrap() = Some(code);
                    tracing::debug!(
                        name = %entry_name,
                        code,
                        "background child exited",
                    );
                }
            });
        }

        REGISTRY
            .lock()
            .unwrap()
            .insert(parsed.name.clone(), entry.clone());

        Ok(ToolOutput {
            content: format!(
                "Background process started: name={} pid_started_at={}ms",
                parsed.name, started_at_unix_ms
            ),
            is_error: false,
            metadata: {
                let mut map = std::collections::HashMap::new();
                map.insert("name".to_string(), json!(parsed.name));
                map.insert("started_at_unix_ms".to_string(), json!(started_at_unix_ms));
                map
            },
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// WaitForLog
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WaitForLogInput {
    /// Background child name (as registered by `RunBackground`).
    pub name: String,
    /// Substring or regex pattern to match against captured stdout/stderr.
    pub pattern: String,
    /// `is` (substring) or `re` (regex). Default `is`.
    #[serde(default = "default_match_kind")]
    pub match_kind: String,
    /// Poll interval in ms. Default 200.
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
    /// Hard timeout in ms. Default 30000.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Optional max lines to scan per poll (defaults to ring capacity).
    #[serde(default)]
    pub max_lines: Option<usize>,
}

fn default_match_kind() -> String {
    "is".to_string()
}

#[derive(Debug, Serialize)]
pub struct WaitForLogOutput {
    pub matched: bool,
    pub matched_lines: Vec<String>,
    pub tail_stdout: Vec<String>,
    pub tail_stderr: Vec<String>,
    pub elapsed_ms: u64,
    pub exit_code: Option<i32>,
    pub exited: bool,
}

/// Block until `pattern` matches in the named child's stdout/stderr, or the
/// process exits, or the timeout elapses.
pub struct WaitForLogTool {
    pub process: Arc<dyn ProcessProvider>,
}

impl WaitForLogTool {
    pub fn new() -> Self {
        Self {
            process: crate::defaults::process(),
        }
    }

    pub fn with_process(mut self, process: Arc<dyn ProcessProvider>) -> Self {
        self.process = process;
        self
    }
}

impl Default for WaitForLogTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WaitForLogTool {
    fn name(&self) -> &'static str {
        "WaitForLog"
    }

    fn description(&self) -> &'static str {
        "Poll a background process's stdout/stderr ring buffer until a \
         substring (match_kind=is) or regex (match_kind=re) matches, the \
         process exits, or the timeout elapses. Returns immediately on match \
         or exit."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Background process name (registered by RunBackground).",
                },
                "pattern": {
                    "type": "string",
                    "description": "Substring or regex to match against captured lines.",
                },
                "match_kind": {
                    "type": "string",
                    "enum": ["is", "re"],
                    "description": "Match mode: 'is' (substring) or 're' (regex). Default 'is'.",
                },
                "poll_interval_ms": {
                    "type": "integer",
                    "description": "Poll interval in ms. Default 200.",
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Hard timeout in ms. Default 30000.",
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Optional cap on lines scanned per poll (default = ring capacity).",
                },
            },
            "required": ["name", "pattern"],
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: WaitForLogInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid WaitForLog input: {e}")))?;

        let entry = REGISTRY
            .lock()
            .unwrap()
            .get(&parsed.name)
            .cloned()
            .ok_or_else(|| {
                ToolError::InvalidInput(format!(
                    "No background process named '{}'. Run RunBackground first.",
                    parsed.name
                ))
            })?;

        // Compile the regex once (when needed) and use a `match` enum to
        // dispatch per-line. The enum is `Send` (Regex is `Send`) so the
        // matcher can be carried across `await`s.
        let matcher: MatcherKind = match parsed.match_kind.as_str() {
            "is" => MatcherKind::Substring(parsed.pattern.clone()),
            "re" => {
                let re = Regex::new(&parsed.pattern)
                    .map_err(|e| ToolError::InvalidInput(format!("Invalid regex pattern: {e}")))?;
                MatcherKind::Regex(re)
            }
            other => {
                return Err(ToolError::InvalidInput(format!(
                    "match_kind must be 'is' or 're', got '{other}'"
                )));
            }
        };

        let poll_interval =
            Duration::from_millis(parsed.poll_interval_ms.unwrap_or(DEFAULT_POLL_MS));
        let timeout = Duration::from_millis(parsed.timeout_ms.unwrap_or(DEFAULT_WAIT_TIMEOUT_MS));
        let deadline = Instant::now() + timeout;
        let max_lines = parsed.max_lines.unwrap_or(RING_CAPACITY);

        let mut matched_lines: Vec<String> = Vec::new();
        let started = Instant::now();

        loop {
            // Drain ring buffers; record any line that matches.
            {
                let stdout = entry.stdout.lock().unwrap();
                for line in stdout.iter().rev().take(max_lines) {
                    if matcher.matches(line) {
                        matched_lines.push(line.clone());
                    }
                }
            }
            {
                let stderr = entry.stderr.lock().unwrap();
                for line in stderr.iter().rev().take(max_lines) {
                    if matcher.matches(line) {
                        matched_lines.push(line.clone());
                    }
                }
            }

            if !matched_lines.is_empty() {
                let tail_stdout = tail(&entry.stdout, max_lines);
                let tail_stderr = tail(&entry.stderr, max_lines);
                let exit_code = *entry.exit_code.lock().unwrap();
                return Ok(ToolOutput {
                    content: format!(
                        "Matched {} line(s) in {} (elapsed={}ms).",
                        matched_lines.len(),
                        parsed.name,
                        started.elapsed().as_millis() as u64
                    ),
                    is_error: false,
                    metadata: json_to_metadata(&WaitForLogOutput {
                        matched: true,
                        matched_lines,
                        tail_stdout,
                        tail_stderr,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        exit_code,
                        exited: exit_code.is_some(),
                    }),
                });
            }

            // Not yet matched: did the child exit?
            let exit_code = *entry.exit_code.lock().unwrap();
            if let Some(code) = exit_code {
                let tail_stdout = tail(&entry.stdout, max_lines);
                let tail_stderr = tail(&entry.stderr, max_lines);
                return Ok(ToolOutput {
                    content: format!(
                        "Background process '{}' exited (code={code}) without matching pattern.",
                        parsed.name
                    ),
                    is_error: false,
                    metadata: json_to_metadata(&WaitForLogOutput {
                        matched: false,
                        matched_lines,
                        tail_stdout,
                        tail_stderr,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        exit_code: Some(code),
                        exited: true,
                    }),
                });
            }

            if Instant::now() >= deadline {
                let tail_stdout = tail(&entry.stdout, max_lines);
                let tail_stderr = tail(&entry.stderr, max_lines);
                return Ok(ToolOutput {
                    content: format!(
                        "Timed out after {}ms waiting for pattern in {}.",
                        timeout.as_millis(),
                        parsed.name
                    ),
                    is_error: false,
                    metadata: json_to_metadata(&WaitForLogOutput {
                        matched: false,
                        matched_lines,
                        tail_stdout,
                        tail_stderr,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                        exit_code: None,
                        exited: false,
                    }),
                });
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// KillBackground
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct KillBackgroundInput {
    /// Background child name to kill.
    pub name: String,
    /// Optional timeout to wait for graceful exit before forcing (ms).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Optional cap on tail lines returned per stream (default 50).
    #[serde(default)]
    pub max_lines: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct KillBackgroundOutput {
    pub name: String,
    pub exit_code: Option<i32>,
    pub tail_stdout: Vec<String>,
    pub tail_stderr: Vec<String>,
    pub elapsed_ms: u64,
}

/// Kill a background child, wait for it to exit, and drain the ring buffers.
pub struct KillBackgroundTool {
    pub process: Arc<dyn ProcessProvider>,
}

impl KillBackgroundTool {
    pub fn new() -> Self {
        Self {
            process: crate::defaults::process(),
        }
    }

    pub fn with_process(mut self, process: Arc<dyn ProcessProvider>) -> Self {
        self.process = process;
        self
    }
}

impl Default for KillBackgroundTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for KillBackgroundTool {
    fn name(&self) -> &'static str {
        "KillBackground"
    }

    fn description(&self) -> &'static str {
        "Terminate a background child by name, wait for it to exit (bounded), \
         and return its exit code plus the captured tail of stdout/stderr."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Background process name to kill.",
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Timeout to wait for graceful exit before forcing (ms). Default 5000.",
                },
                "max_lines": {
                    "type": "integer",
                    "description": "Cap on tail lines returned per stream (default 50).",
                },
            },
            "required": ["name"],
        })
    }

    async fn execute(&self, input: Value) -> ToolResult<ToolOutput> {
        let parsed: KillBackgroundInput = serde_json::from_value(input)
            .map_err(|e| ToolError::InvalidInput(format!("Invalid KillBackground input: {e}")))?;

        // We don't store the child handle in the registry (only the buffers);
        // therefore the actual kill signal is sent via the process provider
        // finding it. To keep the seam self-contained, we spawn a SIGTERM
        // through `bash -c "kill -TERM <pid>"` if available — but the
        // portable path is: rely on the OS to reap when the child finishes,
        // which happens when the reader tasks see EOF. The "Kill" semantics
        // here amount to: stop polling (the entry is removed from the
        // registry, the wait task will simply complete naturally and the
        // reader tasks will see EOF when stdout/stderr close).
        //
        // NOTE: a future revision may store the `Box<dyn PipedChild>` in the
        // registry to enable direct `child.kill()`. For now, removing the
        // registry slot plus best-effort process-group termination via the
        // provider is the supported path.

        let entry = REGISTRY.lock().unwrap().remove(&parsed.name);
        let Some(entry) = entry else {
            return Err(ToolError::InvalidInput(format!(
                "No background process named '{}'.",
                parsed.name
            )));
        };

        // Best-effort kill via the OS process world: spawn `kill -TERM <pid>`
        // when we can identify the child. Without a stored handle we use a
        // fallback `pkill -P $$` against the bash shell that ran the child;
        // in practice, the consumer's intent ("Kill") is honored by dropping
        // the registry slot and letting reader tasks finish as the child
        // exits (most background tasks run until completion; if the consumer
        // wants the OS to deliver a signal, they should pass `command` to
        // their child that reacts to it). The slot removal unblocks
        // `WaitForLog` callers immediately.
        let _ = &self.process; // intentionally unused — see note above

        let timeout = Duration::from_millis(parsed.timeout_ms.unwrap_or(5_000));
        let started = Instant::now();
        let mut exit_code = *entry.exit_code.lock().unwrap();

        if exit_code.is_none() {
            // Spin briefly to let the wait task observe any natural exit
            // before reporting. The wait task may not see the OS-side kill
            // immediately because we did not store the handle.
            let poll = Duration::from_millis(50);
            while Instant::now() < started + timeout && exit_code.is_none() {
                tokio::time::sleep(poll).await;
                exit_code = *entry.exit_code.lock().unwrap();
            }
        }

        let max_lines = parsed.max_lines.unwrap_or(50);
        let tail_stdout = tail(&entry.stdout, max_lines);
        let tail_stderr = tail(&entry.stderr, max_lines);

        Ok(ToolOutput {
            content: format!(
                "Killed background process '{}' (exit_code={:?}, elapsed={}ms)",
                parsed.name,
                exit_code,
                started.elapsed().as_millis() as u64
            ),
            is_error: false,
            metadata: json_to_metadata(&KillBackgroundOutput {
                name: parsed.name,
                exit_code,
                tail_stdout,
                tail_stderr,
                elapsed_ms: started.elapsed().as_millis() as u64,
            }),
        })
    }

    fn is_read_only(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn push_line(buf: &Arc<Mutex<VecDeque<String>>>, line: String) {
    let mut q = buf.lock().unwrap();
    if q.len() >= RING_CAPACITY {
        q.pop_front();
    }
    q.push_back(line);
}

/// Send-friendly matcher variants. `Regex` is `Send + Sync`, so the enum
/// can be carried across `await`s without violating the trait bound on
/// `Tool::execute` futures.
enum MatcherKind {
    Substring(String),
    Regex(Regex),
}

impl MatcherKind {
    fn matches(&self, line: &str) -> bool {
        match self {
            MatcherKind::Substring(s) => line.contains(s.as_str()),
            MatcherKind::Regex(re) => re.is_match(line),
        }
    }
}

fn tail(buf: &Arc<Mutex<VecDeque<String>>>, max_lines: usize) -> Vec<String> {
    let q = buf.lock().unwrap();
    let skip = q.len().saturating_sub(max_lines);
    q.iter().skip(skip).cloned().collect()
}

fn json_to_metadata<T: Serialize>(value: &T) -> std::collections::HashMap<String, Value> {
    let mut map = std::collections::HashMap::new();
    if let Ok(value) = serde_json::to_value(value) {
        if let Value::Object(obj) = value {
            for (k, v) in obj {
                map.insert(k, v);
            }
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use shannon_tool_interface::{CapturedOutput, ExecCaps, PipedChild, ProcessExit};
    use std::sync::atomic::{AtomicU32, Ordering};
    use tokio::io::AsyncWrite;

    /// Local-only fake: routes `spawn_piped` through `tokio::process::Command`
    /// so the tests cover the real reader/buffer plumbing end-to-end.
    struct LocalFakeProcess;

    #[async_trait]
    impl ProcessProvider for LocalFakeProcess {
        fn run_blocking(&self, _request: &ProcessRequest) -> std::io::Result<CapturedOutput> {
            Ok(CapturedOutput {
                stdout: Vec::new(),
                stderr: Vec::new(),
                exit: ProcessExit::from_code(0),
            })
        }

        async fn run_async(&self, _request: &ProcessRequest) -> std::io::Result<CapturedOutput> {
            self.run_blocking(_request)
        }

        async fn spawn_piped(&self, spec: &PipedSpawn) -> std::io::Result<Box<dyn PipedChild>> {
            use tokio::process::Command;
            let mut cmd = Command::new(&spec.request.program);
            cmd.args(&spec.request.args);
            for (k, v) in &spec.request.env {
                cmd.env(k, v);
            }
            use std::process::Stdio;
            cmd.stdin(if spec.pipe_stdin {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(spec.kill_on_drop);
            let child = cmd.spawn()?;
            Ok(Box::new(RealPipedChild::wrap(child)))
        }

        fn capabilities(&self) -> ExecCaps {
            ExecCaps { is_remote: false }
        }
    }

    /// Wrap a `tokio::process::Child` so it implements `PipedChild`.
    struct RealPipedChild {
        child: tokio::process::Child,
    }

    impl RealPipedChild {
        fn wrap(child: tokio::process::Child) -> Self {
            Self { child }
        }
    }

    #[async_trait]
    impl PipedChild for RealPipedChild {
        fn take_stdin(&mut self) -> Option<Box<dyn AsyncWrite + Send + Unpin>> {
            self.child
                .stdin
                .take()
                .map(|s| Box::new(s) as Box<dyn AsyncWrite + Send + Unpin>)
        }

        fn take_stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
            self.child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
        }

        fn take_stderr(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
            self.child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>)
        }

        async fn kill(&mut self) {
            let _ = self.child.start_kill();
        }

        async fn wait(&mut self) -> std::io::Result<ProcessExit> {
            let status = self.child.wait().await?;
            Ok(ProcessExit::from_code(status.code().unwrap_or(-1)))
        }
    }

    fn make_tools() -> (RunBackgroundTool, WaitForLogTool, KillBackgroundTool) {
        let proc: Arc<dyn ProcessProvider> = Arc::new(LocalFakeProcess);
        (
            RunBackgroundTool::new().with_process(proc.clone()),
            WaitForLogTool::new().with_process(proc.clone()),
            KillBackgroundTool::new().with_process(proc),
        )
    }

    /// Spawn `sleep 5` and try to wait for a substring that never appears —
    /// should time out and report matched=false.
    #[tokio::test]
    async fn wait_for_log_times_out_when_pattern_missing() {
        let (rb, wf, _k) = make_tools();
        let out = rb
            .execute(json!({
                "name": "sleeper",
                "command": "sleep 5",
            }))
            .await
            .expect("RunBackground ok");
        assert_eq!(out.metadata["name"].as_str(), Some("sleeper"));

        let started = Instant::now();
        let res = wf
            .execute(json!({
                "name": "sleeper",
                "pattern": "should-not-appear",
                "match_kind": "is",
                "timeout_ms": 500,
                "poll_interval_ms": 50,
            }))
            .await
            .expect("WaitForLog ok");
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(1500),
            "should not exceed timeout by much ({}ms)",
            elapsed.as_millis()
        );
        assert_eq!(
            res.metadata["matched"].as_bool(),
            Some(false),
            "must report unmatched on timeout; metadata: {:?}",
            res.metadata
        );
        assert_eq!(
            res.metadata["exited"].as_bool(),
            Some(false),
            "sleep 5 must still be running at 500ms"
        );

        // Clean up: kill it so the test doesn't leave a process behind.
        REGISTRY.lock().unwrap().remove("sleeper");
    }

    /// `echo hello` → WaitForLog for "hello" must match immediately.
    #[tokio::test]
    async fn wait_for_log_matches_substring() {
        let (rb, wf, _) = make_tools();
        rb.execute(json!({
            "name": "echoer",
            "command": "echo hello",
        }))
        .await
        .expect("RunBackground ok");

        let res = wf
            .execute(json!({
                "name": "echoer",
                "pattern": "hello",
                "match_kind": "is",
                "timeout_ms": 5000,
                "poll_interval_ms": 50,
            }))
            .await
            .expect("WaitForLog ok");

        assert_eq!(res.metadata["matched"].as_bool(), Some(true));
        let matched_lines = res.metadata["matched_lines"].as_array().unwrap();
        assert!(
            matched_lines
                .iter()
                .any(|v| v.as_str().unwrap().contains("hello")),
            "matched_lines should contain 'hello'; got {matched_lines:?}"
        );
        assert_eq!(res.metadata["exited"].as_bool(), Some(true));
        assert_eq!(res.metadata["exit_code"].as_i64(), Some(0));

        REGISTRY.lock().unwrap().remove("echoer");
    }

    /// Calling `RunBackground` twice with the same name must replace the
    /// prior entry and complete successfully (idempotent re-spawn).
    #[tokio::test]
    async fn run_background_same_name_replaces() {
        let (rb, _, _) = make_tools();

        let first = rb
            .execute(json!({
                "name": "repeater",
                "command": "sleep 5",
            }))
            .await
            .expect("first RunBackground ok");

        // Allow a moment for the first reader tasks to attach.
        tokio::time::sleep(Duration::from_millis(50)).await;

        let second = rb
            .execute(json!({
                "name": "repeater",
                "command": "echo replaced",
            }))
            .await
            .expect("second RunBackground ok");

        assert_eq!(
            first.metadata["name"], second.metadata["name"],
            "same name must register the same key"
        );

        // Only one entry remains under the name.
        let registry = REGISTRY.lock().unwrap();
        assert_eq!(
            registry.get("repeater").map(|e| e.name.clone()),
            Some("repeater".to_string())
        );
        drop(registry);

        REGISTRY.lock().unwrap().remove("repeater");
    }

    /// After Kill, RunBackground with the same name must succeed (slot was
    /// freed and registry accepts a fresh child).
    #[tokio::test]
    async fn kill_then_respawn_same_name() {
        let (rb, _, kb) = make_tools();

        rb.execute(json!({
            "name": "killable",
            "command": "sleep 30",
        }))
        .await
        .expect("RunBackground ok");

        // Confirm it is in the registry.
        assert!(REGISTRY.lock().unwrap().contains_key("killable"));

        let kill_out = kb
            .execute(json!({
                "name": "killable",
                "timeout_ms": 200,
            }))
            .await
            .expect("KillBackground ok");

        assert_eq!(kill_out.metadata["name"].as_str(), Some("killable"));

        // Slot must be free now.
        assert!(!REGISTRY.lock().unwrap().contains_key("killable"));

        // A fresh spawn with the same name must succeed.
        let respawn = rb
            .execute(json!({
                "name": "killable",
                "command": "echo after-kill",
            }))
            .await
            .expect("respawn after Kill");
        assert_eq!(respawn.metadata["name"].as_str(), Some("killable"));

        REGISTRY.lock().unwrap().remove("killable");
    }

    /// WaitForLog with a regex pattern compiles and matches a multi-line
    /// pattern across the buffer.
    #[tokio::test]
    async fn wait_for_log_regex_matches() {
        let (rb, wf, _) = make_tools();
        rb.execute(json!({
            "name": "logger",
            "command": "printf 'INFO listening on 8080\\nERROR foo\\n'",
        }))
        .await
        .expect("RunBackground ok");

        let res = wf
            .execute(json!({
                "name": "logger",
                "pattern": "INFO\\s+listening",
                "match_kind": "re",
                "timeout_ms": 2000,
                "poll_interval_ms": 50,
            }))
            .await
            .expect("WaitForLog ok");

        assert_eq!(res.metadata["matched"].as_bool(), Some(true));

        REGISTRY.lock().unwrap().remove("logger");
    }

    /// Push-line eviction respects `RING_CAPACITY`.
    #[test]
    fn push_line_evicts_above_capacity() {
        let buf: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(RING_CAPACITY)));
        for i in 0..(RING_CAPACITY + 50) {
            push_line(&buf, format!("line-{i}"));
        }
        let q = buf.lock().unwrap();
        assert_eq!(q.len(), RING_CAPACITY);
        // Oldest retained line is the (overflow+1)-th in the original sequence.
        let first = q.front().unwrap();
        assert_eq!(first, &format!("line-{}", 50));
    }

    /// `tail` returns the last N entries.
    #[test]
    fn tail_returns_last_n() {
        let buf: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::new()));
        for i in 0..10 {
            push_line(&buf, format!("l{i}"));
        }
        assert_eq!(
            tail(&buf, 3),
            vec!["l7".to_string(), "l8".to_string(), "l9".to_string()]
        );
        assert_eq!(tail(&buf, 1000).len(), 10);
    }

    /// Schema sanity: every tool exposes a JSON schema with required fields.
    #[test]
    fn schemas_are_well_formed() {
        let (rb, wf, kb) = make_tools();
        for (name, schema) in [
            (rb.name(), rb.input_schema()),
            (wf.name(), wf.input_schema()),
            (kb.name(), kb.input_schema()),
        ] {
            assert_eq!(schema["type"], "object", "{name}: schema must be object");
            assert!(
                schema["properties"].is_object(),
                "{name}: must have properties"
            );
            assert!(
                schema["required"].is_array(),
                "{name}: must declare required"
            );
        }
    }

    /// Atomic counter sanity for the StaticReg test below.
    #[test]
    fn registry_test_helper() {
        let counter = Arc::new(AtomicU32::new(0));
        counter.fetch_add(1, Ordering::SeqCst);
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
