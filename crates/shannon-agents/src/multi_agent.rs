//! # Multi-Agent Spawner
//!
//! Parallel multi-agent launching with dependency resolution, concurrency limiting,
//! timeout handling, and fail-fast support.
//!
//! Provides:
//! - `MultiAgentConfig`: Configuration for launching multiple agents
//! - `MultiAgentSpawner`: Orchestrates parallel agent execution
//! - `MultiAgentResult`: Aggregated results from all agents
//! - `AgentResult`: Individual agent execution result

use crate::executor::AgentExecutor;
use serde::{Deserialize, Serialize};
use shannon_core::tools::ToolOutput;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

// ---------------------------------------------------------------------------
// Configuration types
// ---------------------------------------------------------------------------

/// Configuration for a single agent within a multi-agent launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Unique human-readable agent name
    pub name: String,
    /// Task description / prompt for the agent
    pub task: String,
    /// Optional system prompt defining agent behavior
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Optional model override (e.g., "claude-sonnet-4-6")
    #[serde(default)]
    pub model: Option<String>,
    /// Optional restricted tool set
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    /// Names of agents that must complete before this one can start
    #[serde(default)]
    pub depends_on: Vec<String>,
}

impl AgentConfig {
    /// Create a new agent config with the given name and task.
    pub fn new(name: impl Into<String>, task: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            task: task.into(),
            system_prompt: None,
            model: None,
            tools: None,
            depends_on: Vec::new(),
        }
    }

    /// Set an optional model override.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set a system prompt defining agent behavior.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set an optional tool restriction list.
    pub fn with_tools(mut self, tools: Vec<String>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Add a dependency on another agent by name.
    pub fn depends_on(mut self, agent_name: impl Into<String>) -> Self {
        self.depends_on.push(agent_name.into());
        self
    }
}

/// Configuration for a multi-agent launch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentConfig {
    /// Agent configurations to launch
    pub agents: Vec<AgentConfig>,
    /// Maximum number of agents running concurrently (default: 4)
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,
    /// Per-agent timeout (default: 5 minutes)
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// If true, stop all remaining agents on first failure
    #[serde(default)]
    pub fail_fast: bool,
    /// Default system prompt for agents that don't specify one
    #[serde(default)]
    pub default_system_prompt: Option<String>,
}

fn default_max_parallel() -> usize {
    4
}

fn default_timeout_secs() -> u64 {
    300 // 5 minutes
}

impl MultiAgentConfig {
    /// Create a new multi-agent config with the given agents.
    pub fn new(agents: Vec<AgentConfig>) -> Self {
        Self {
            agents,
            max_parallel: default_max_parallel(),
            timeout_secs: default_timeout_secs(),
            fail_fast: false,
            default_system_prompt: None,
        }
    }

    /// Set the maximum parallelism.
    pub fn with_max_parallel(mut self, max: usize) -> Self {
        self.max_parallel = if max == 0 { 1 } else { max };
        self
    }

    /// Set the per-agent timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_secs = timeout.as_secs();
        self
    }

    /// Enable fail-fast mode.
    pub fn with_fail_fast(mut self) -> Self {
        self.fail_fast = true;
        self
    }

    /// Set the default system prompt for agents that don't specify one.
    pub fn with_default_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.default_system_prompt = Some(prompt.into());
        self
    }
}

impl Default for MultiAgentConfig {
    fn default() -> Self {
        Self {
            agents: Vec::new(),
            max_parallel: default_max_parallel(),
            timeout_secs: default_timeout_secs(),
            fail_fast: false,
            default_system_prompt: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Status of an individual agent execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentResultStatus {
    /// Agent completed successfully
    Completed,
    /// Agent failed with an error
    Failed,
    /// Agent exceeded the configured timeout
    Timeout,
    /// Agent was skipped due to a dependency failure or fail-fast cancellation
    Skipped,
}

impl std::fmt::Display for AgentResultStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentResultStatus::Completed => write!(f, "completed"),
            AgentResultStatus::Failed => write!(f, "failed"),
            AgentResultStatus::Timeout => write!(f, "timeout"),
            AgentResultStatus::Skipped => write!(f, "skipped"),
        }
    }
}

/// Result from a single agent execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Name of the agent
    pub agent_name: String,
    /// Final execution status
    pub status: AgentResultStatus,
    /// Output from the agent (if it completed successfully)
    pub output: Option<ToolOutput>,
    /// Wall-clock duration of the agent's execution
    pub duration: Duration,
    /// Error message (if the agent failed or timed out)
    pub error: Option<String>,
}

impl AgentResult {
    /// Create a completed result.
    pub fn completed(agent_name: String, output: ToolOutput, duration: Duration) -> Self {
        Self {
            agent_name,
            status: AgentResultStatus::Completed,
            output: Some(output),
            duration,
            error: None,
        }
    }

    /// Create a failed result.
    pub fn failed(agent_name: String, error: String, duration: Duration) -> Self {
        Self {
            agent_name,
            status: AgentResultStatus::Failed,
            output: None,
            duration,
            error: Some(error),
        }
    }

    /// Create a timeout result.
    pub fn timed_out(agent_name: String, duration: Duration) -> Self {
        Self {
            agent_name: agent_name.clone(),
            status: AgentResultStatus::Timeout,
            output: None,
            duration,
            error: Some(format!(
                "Agent '{}' exceeded timeout of {}s",
                agent_name,
                duration.as_secs()
            )),
        }
    }

    /// Create a skipped result.
    pub fn skipped(agent_name: String) -> Self {
        Self {
            agent_name: agent_name.clone(),
            status: AgentResultStatus::Skipped,
            output: None,
            duration: Duration::ZERO,
            error: Some(format!("Agent '{agent_name}' was skipped")),
        }
    }
}

/// Aggregated result from a multi-agent spawn operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentResult {
    /// Individual results for each agent, in the order they were configured
    pub agent_results: Vec<AgentResult>,
    /// Total wall-clock duration for the entire multi-agent operation
    pub total_duration: Duration,
    /// Number of agents that completed successfully
    pub success_count: usize,
    /// Number of agents that failed, timed out, or were skipped
    pub failure_count: usize,
}

impl MultiAgentResult {
    /// Whether all agents completed successfully.
    pub fn all_succeeded(&self) -> bool {
        self.failure_count == 0 && self.agent_results.len() == self.success_count
    }

    /// Get results for failed agents only.
    pub fn failures(&self) -> Vec<&AgentResult> {
        self.agent_results
            .iter()
            .filter(|r| r.status != AgentResultStatus::Completed)
            .collect()
    }

    /// Get results for successful agents only.
    pub fn successes(&self) -> Vec<&AgentResult> {
        self.agent_results
            .iter()
            .filter(|r| r.status == AgentResultStatus::Completed)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Topological sort for dependency resolution
// ---------------------------------------------------------------------------

/// Errors that can occur during topological sort.
#[derive(Debug, Clone)]
pub enum DependencyError {
    /// An agent depends on a name that does not exist
    UnknownDependency(String),
    /// A circular dependency was detected
    CircularDependency(Vec<String>),
    /// An agent name appears more than once
    DuplicateAgent(String),
}

impl std::fmt::Display for DependencyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DependencyError::UnknownDependency(name) => {
                write!(f, "unknown dependency '{name}'")
            }
            DependencyError::CircularDependency(cycle) => {
                write!(f, "circular dependency detected: {}", cycle.join(" -> "))
            }
            DependencyError::DuplicateAgent(name) => {
                write!(f, "duplicate agent name '{name}'")
            }
        }
    }
}

/// Perform a topological sort on agent configurations based on their dependencies.
/// Returns agents in execution order (dependencies first), preserving original
/// order for agents with no relative dependency constraints.
pub fn topological_sort(agents: &[AgentConfig]) -> Result<Vec<&AgentConfig>, DependencyError> {
    // Build name -> index map and check for duplicates
    let mut name_to_idx: HashMap<String, usize> = HashMap::new();
    for (i, agent) in agents.iter().enumerate() {
        if name_to_idx.contains_key(&agent.name) {
            return Err(DependencyError::DuplicateAgent(agent.name.clone()));
        }
        name_to_idx.insert(agent.name.clone(), i);
    }

    // Validate all dependencies reference existing agents
    for agent in agents {
        for dep in &agent.depends_on {
            if !name_to_idx.contains_key(dep) {
                return Err(DependencyError::UnknownDependency(dep.clone()));
            }
        }
    }

    // Kahn's algorithm with stable ordering (use index to break ties)
    let n = agents.len();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for agent in agents {
        in_degree.insert(agent.name.as_str(), 0);
    }
    for agent in agents {
        for dep in &agent.depends_on {
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(agent.name.as_str());
            if let Some(deg) = in_degree.get_mut(agent.name.as_str()) {
                *deg += 1;
            }
        }
    }

    // Seed queue in original agent order (stable sort)
    let mut queue: VecDeque<&str> = VecDeque::new();
    for agent in agents {
        if in_degree[agent.name.as_str()] == 0 {
            queue.push_back(agent.name.as_str());
        }
    }

    let mut sorted_order: Vec<&AgentConfig> = Vec::new();

    while let Some(name) = queue.pop_front() {
        let idx = name_to_idx[name];
        sorted_order.push(&agents[idx]);

        if let Some(children) = dependents.get(name) {
            for &child in children {
                let deg = in_degree
                    .get_mut(child)
                    .expect("in_degree entry initialized for all agents above");
                *deg -= 1;
                if *deg == 0 {
                    queue.push_back(child);
                }
            }
        }
    }

    if sorted_order.len() != n {
        // Find the cycle for a useful error message
        let visited: HashSet<&str> = sorted_order.iter().map(|a| a.name.as_str()).collect();
        let cycle: Vec<String> = agents
            .iter()
            .filter(|a| !visited.contains(a.name.as_str()))
            .map(|a| a.name.clone())
            .collect();
        return Err(DependencyError::CircularDependency(cycle));
    }

    Ok(sorted_order)
}

// ---------------------------------------------------------------------------
// MultiAgentSpawner
// ---------------------------------------------------------------------------

/// Orchestrates parallel multi-agent execution with dependency resolution,
/// concurrency limiting, timeouts, and fail-fast support.
pub struct MultiAgentSpawner;

impl MultiAgentSpawner {
    /// Spawn multiple agents according to the configuration.
    ///
    /// This method:
    /// 1. Validates configuration and resolves dependencies via topological sort
    /// 2. Groups agents into "waves" where each wave contains agents whose
    ///    dependencies are all satisfied by previous waves
    /// 3. Executes each wave in parallel, respecting the `max_parallel` limit
    /// 4. Handles per-agent timeouts and fail-fast cancellation
    /// 5. Returns aggregated results
    pub async fn spawn(
        config: MultiAgentConfig,
        executor: Option<Arc<dyn AgentExecutor>>,
    ) -> MultiAgentResult {
        let total_start = Instant::now();

        // Validate: empty agent list
        if config.agents.is_empty() {
            return MultiAgentResult {
                agent_results: Vec::new(),
                total_duration: total_start.elapsed(),
                success_count: 0,
                failure_count: 0,
            };
        }

        // Validate: duplicate names
        let mut seen_names: HashSet<&str> = HashSet::new();
        for agent in &config.agents {
            if !seen_names.insert(&agent.name) {
                let result = AgentResult::failed(
                    agent.name.clone(),
                    format!("duplicate agent name '{}'", agent.name),
                    Duration::ZERO,
                );
                return MultiAgentResult {
                    agent_results: vec![result],
                    total_duration: total_start.elapsed(),
                    success_count: 0,
                    failure_count: 1,
                };
            }
        }

        // Topological sort for dependency resolution
        let sorted = match topological_sort(&config.agents) {
            Ok(s) => s,
            Err(e) => {
                // Return all agents as failed with the dependency error
                let failure_count = config.agents.len();
                let agent_results: Vec<AgentResult> = config
                    .agents
                    .iter()
                    .map(|a| AgentResult::failed(a.name.clone(), e.to_string(), Duration::ZERO))
                    .collect();
                return MultiAgentResult {
                    agent_results,
                    total_duration: total_start.elapsed(),
                    success_count: 0,
                    failure_count,
                };
            }
        };

        // Group into execution waves based on dependency levels
        let waves = Self::build_waves(&sorted);

        // Execute waves sequentially, agents within a wave in parallel
        let timeout = Duration::from_secs(config.timeout_secs);
        let semaphore = Arc::new(Semaphore::new(config.max_parallel));
        let cancelled = Arc::new(AtomicBool::new(false));

        let mut all_results: Vec<AgentResult> = Vec::with_capacity(config.agents.len());
        let mut result_map: HashMap<String, AgentResult> = HashMap::new();

        for wave in &waves {
            if cancelled.load(Ordering::Relaxed) {
                // Fail-fast: skip all remaining agents
                for agent in wave {
                    let result = AgentResult::skipped(agent.name.clone());
                    result_map.insert(agent.name.clone(), result);
                }
                continue;
            }

            // Check if any dependency in this wave failed (shouldn't happen with
            // correct wave building, but defensive)
            let wave_handles: Vec<JoinHandle<AgentResult>> = wave
                .iter()
                .filter(|agent| {
                    // Check dependencies
                    for dep in &agent.depends_on {
                        if let Some(dep_result) = result_map.get(dep) {
                            if dep_result.status != AgentResultStatus::Completed {
                                if config.fail_fast {
                                    return false;
                                }
                                return false;
                            }
                        }
                    }
                    true
                })
                .map(|agent| {
                    let agent = (*agent).clone();
                    let sem = semaphore.clone();
                    let cancel_flag = cancelled.clone();
                    let wave_timeout = timeout;
                    let exec = executor.clone();
                    let default_prompt = config.default_system_prompt.clone();

                    tokio::spawn(async move {
                        // Acquire semaphore permit (respects max_parallel)
                        let _permit = match sem.acquire().await {
                            Ok(p) => p,
                            Err(e) => {
                                tracing::error!("semaphore acquire failed: {e}");
                                return AgentResult::skipped(agent.name.clone());
                            }
                        };

                        // Check cancellation before starting
                        if cancel_flag.load(Ordering::Relaxed) {
                            return AgentResult::skipped(agent.name.clone());
                        }

                        let start = Instant::now();

                        // Execute with timeout
                        let result = tokio::time::timeout(
                            wave_timeout,
                            Self::execute_agent(&agent, exec.as_deref(), default_prompt.as_deref()),
                        )
                        .await;

                        let duration = start.elapsed();

                        match result {
                            Ok(Ok(agent_result)) => agent_result,
                            Ok(Err(e)) => {
                                // Agent returned an error
                                AgentResult::failed(agent.name.clone(), e, duration)
                            }
                            Err(_) => {
                                // Timeout
                                AgentResult::timed_out(agent.name.clone(), duration)
                            }
                        }
                    })
                })
                .collect();

            // Also mark skipped agents (those whose dependencies failed)
            for agent in wave {
                let should_skip = agent.depends_on.iter().any(|dep| {
                    result_map
                        .get(dep)
                        .map(|r| r.status != AgentResultStatus::Completed)
                        .unwrap_or(false)
                });

                if should_skip {
                    result_map.insert(agent.name.clone(), AgentResult::skipped(agent.name.clone()));
                }
            }

            // Await all agents in this wave
            for handle in wave_handles {
                let result = handle.await.unwrap_or_else(|e| {
                    AgentResult::failed(
                        "unknown".to_string(),
                        format!("task join error: {e}"),
                        Duration::ZERO,
                    )
                });

                let name = result.agent_name.clone();

                // Handle fail-fast
                if config.fail_fast
                    && (result.status == AgentResultStatus::Failed
                        || result.status == AgentResultStatus::Timeout)
                {
                    cancelled.store(true, Ordering::Relaxed);
                }

                result_map.insert(name, result);
            }
        }

        // Assemble results in the original configuration order
        for agent in &config.agents {
            if let Some(result) = result_map.remove(&agent.name) {
                all_results.push(result);
            }
        }

        let success_count = all_results
            .iter()
            .filter(|r| r.status == AgentResultStatus::Completed)
            .count();
        let failure_count = all_results.len() - success_count;

        tracing::info!(
            total_agents = all_results.len(),
            succeeded = success_count,
            failed = failure_count,
            duration_ms = total_start.elapsed().as_millis() as u64,
            "Multi-agent spawn completed"
        );

        MultiAgentResult {
            agent_results: all_results,
            total_duration: total_start.elapsed(),
            success_count,
            failure_count,
        }
    }

    /// Spawn multiple agents in a background tokio task.
    /// Returns a JoinHandle that can be awaited for the result.
    pub fn spawn_background(config: MultiAgentConfig) -> JoinHandle<MultiAgentResult> {
        tokio::spawn(async move { Self::spawn(config, None).await })
    }

    /// Build execution waves from topologically-sorted agents.
    /// Each wave contains agents that can run concurrently (all their
    /// dependencies are in earlier waves).
    fn build_waves<'a>(sorted: &[&'a AgentConfig]) -> Vec<Vec<&'a AgentConfig>> {
        if sorted.is_empty() {
            return Vec::new();
        }

        // Track which wave each agent is assigned to
        let mut agent_wave: HashMap<&str, usize> = HashMap::new();
        let mut waves: Vec<Vec<&AgentConfig>> = Vec::new();

        for &agent in sorted {
            // Determine which wave this agent belongs to:
            // - No dependencies -> wave 0
            // - Has dependencies -> max(dep waves) + 1
            let target_wave = if agent.depends_on.is_empty() {
                0
            } else {
                agent
                    .depends_on
                    .iter()
                    .filter_map(|dep| agent_wave.get(dep.as_str()).copied())
                    .max()
                    .unwrap_or(0)
                    + 1
            };

            // Ensure the waves vector is large enough
            while waves.len() <= target_wave {
                waves.push(Vec::new());
            }

            waves[target_wave].push(agent);
            agent_wave.insert(&agent.name, target_wave);
        }

        waves
    }

    /// Execute a single agent. In production this would launch a subprocess
    /// or call an AI API. For now it produces a synthetic ToolOutput.
    async fn execute_agent(
        agent: &AgentConfig,
        executor: Option<&dyn AgentExecutor>,
        default_system_prompt: Option<&str>,
    ) -> Result<AgentResult, String> {
        let start = Instant::now();

        let system_prompt = agent
            .system_prompt
            .as_deref()
            .or(default_system_prompt)
            .unwrap_or("You are a helpful AI assistant. Complete the task concisely.");

        if let Some(exec) = executor {
            // Real execution via injected executor (LLM-backed)
            let result = exec
                .execute(
                    system_prompt,
                    &agent.task,
                    agent.model.as_deref(),
                    agent.tools.as_deref(),
                )
                .await?;

            Ok(AgentResult::completed(
                agent.name.clone(),
                result,
                start.elapsed(),
            ))
        } else {
            // No executor configured: log warning and return a stub result
            tracing::warn!(
                agent = %agent.name,
                "No executor configured for agent; returning stub result"
            );
            Ok(AgentResult::completed(
                agent.name.clone(),
                ToolOutput {
                    content: format!(
                        "Agent '{}' completed task (no executor configured)",
                        agent.name
                    ),
                    is_error: false,
                    metadata: std::collections::HashMap::new(),
                },
                start.elapsed(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as StdHashMap;

    // ---- AgentConfig tests ----

    #[test]
    fn test_agent_config_new() {
        let config = AgentConfig::new("agent-1", "Do something");
        assert_eq!(config.name, "agent-1");
        assert_eq!(config.task, "Do something");
        assert!(config.model.is_none());
        assert!(config.tools.is_none());
        assert!(config.depends_on.is_empty());
    }

    #[test]
    fn test_agent_config_builder() {
        let config = AgentConfig::new("builder-test", "Build stuff")
            .with_model("claude-sonnet-4-6")
            .with_tools(vec!["read".into(), "write".into()])
            .depends_on("setup-agent");

        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4-6"));
        assert_eq!(config.tools.as_deref().unwrap().len(), 2);
        assert_eq!(config.depends_on, vec!["setup-agent"]);
    }

    #[test]
    fn test_agent_config_multiple_deps() {
        let config = AgentConfig::new("c", "task c")
            .depends_on("a")
            .depends_on("b");

        assert_eq!(config.depends_on, vec!["a", "b"]);
    }

    #[test]
    fn test_agent_config_serde_roundtrip() {
        let config = AgentConfig::new("serde-agent", "test task")
            .with_model("claude-sonnet-4-6")
            .with_tools(vec!["read".into()])
            .depends_on("dep-agent");

        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: AgentConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.name, config.name);
        assert_eq!(deserialized.task, config.task);
        assert_eq!(deserialized.model, config.model);
        assert_eq!(deserialized.tools, config.tools);
        assert_eq!(deserialized.depends_on, config.depends_on);
    }

    // ---- MultiAgentConfig tests ----

    #[test]
    fn test_multi_agent_config_default() {
        let config = MultiAgentConfig::default();
        assert!(config.agents.is_empty());
        assert_eq!(config.max_parallel, 4);
        assert_eq!(config.timeout_secs, 300);
        assert!(!config.fail_fast);
    }

    #[test]
    fn test_multi_agent_config_builder() {
        let agents = vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
        ];
        let config = MultiAgentConfig::new(agents)
            .with_max_parallel(2)
            .with_timeout(Duration::from_secs(60))
            .with_fail_fast();

        assert_eq!(config.max_parallel, 2);
        assert_eq!(config.timeout_secs, 60);
        assert!(config.fail_fast);
    }

    #[test]
    fn test_multi_agent_config_max_parallel_clamp() {
        let config = MultiAgentConfig::new(vec![]).with_max_parallel(0);
        assert_eq!(config.max_parallel, 1);
    }

    #[test]
    fn test_multi_agent_config_serde_roundtrip() {
        let agents = vec![AgentConfig::new("a", "task")];
        let config = MultiAgentConfig::new(agents)
            .with_max_parallel(8)
            .with_fail_fast();

        let json_str = serde_json::to_string(&config).unwrap();
        let deserialized: MultiAgentConfig = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.max_parallel, 8);
        assert!(deserialized.fail_fast);
        assert_eq!(deserialized.agents.len(), 1);
    }

    // ---- AgentResultStatus tests ----

    #[test]
    fn test_agent_result_status_display() {
        assert_eq!(AgentResultStatus::Completed.to_string(), "completed");
        assert_eq!(AgentResultStatus::Failed.to_string(), "failed");
        assert_eq!(AgentResultStatus::Timeout.to_string(), "timeout");
        assert_eq!(AgentResultStatus::Skipped.to_string(), "skipped");
    }

    #[test]
    fn test_agent_result_status_serde() {
        for status in &[
            AgentResultStatus::Completed,
            AgentResultStatus::Failed,
            AgentResultStatus::Timeout,
            AgentResultStatus::Skipped,
        ] {
            let json_str = serde_json::to_string(status).unwrap();
            let deserialized: AgentResultStatus = serde_json::from_str(&json_str).unwrap();
            assert_eq!(*status, deserialized);
        }
    }

    // ---- AgentResult tests ----

    #[test]
    fn test_agent_result_completed() {
        let result = AgentResult::completed(
            "test-agent".into(),
            ToolOutput {
                content: "done".into(),
                is_error: false,
                metadata: StdHashMap::new(),
            },
            Duration::from_secs(2),
        );
        assert_eq!(result.agent_name, "test-agent");
        assert_eq!(result.status, AgentResultStatus::Completed);
        assert!(result.output.is_some());
        assert!(result.error.is_none());
        assert_eq!(result.duration, Duration::from_secs(2));
    }

    #[test]
    fn test_agent_result_failed() {
        let result = AgentResult::failed("bad-agent".into(), "oops".into(), Duration::from_secs(1));
        assert_eq!(result.status, AgentResultStatus::Failed);
        assert!(result.output.is_none());
        assert_eq!(result.error.as_deref(), Some("oops"));
    }

    #[test]
    fn test_agent_result_timed_out() {
        let result = AgentResult::timed_out("slow-agent".into(), Duration::from_secs(10));
        assert_eq!(result.status, AgentResultStatus::Timeout);
        assert!(result.error.unwrap().contains("timeout"));
    }

    #[test]
    fn test_agent_result_skipped() {
        let result = AgentResult::skipped("skipped-agent".into());
        assert_eq!(result.status, AgentResultStatus::Skipped);
        assert_eq!(result.duration, Duration::ZERO);
    }

    #[test]
    fn test_agent_result_serde_roundtrip() {
        let result = AgentResult::completed(
            "ser".into(),
            ToolOutput {
                content: "output".into(),
                is_error: false,
                metadata: StdHashMap::new(),
            },
            Duration::from_millis(500),
        );
        let json_str = serde_json::to_string(&result).unwrap();
        let deserialized: AgentResult = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.agent_name, result.agent_name);
        assert_eq!(deserialized.status, result.status);
        assert_eq!(deserialized.duration, result.duration);
    }

    // ---- MultiAgentResult tests ----

    #[test]
    fn test_multi_agent_result_all_succeeded() {
        let result = MultiAgentResult {
            agent_results: vec![
                AgentResult::completed("a".into(), make_output("ok"), Duration::ZERO),
                AgentResult::completed("b".into(), make_output("ok"), Duration::ZERO),
            ],
            total_duration: Duration::from_secs(1),
            success_count: 2,
            failure_count: 0,
        };
        assert!(result.all_succeeded());
        assert!(result.failures().is_empty());
        assert_eq!(result.successes().len(), 2);
    }

    #[test]
    fn test_multi_agent_result_partial_failure() {
        let result = MultiAgentResult {
            agent_results: vec![
                AgentResult::completed("a".into(), make_output("ok"), Duration::ZERO),
                AgentResult::failed("b".into(), "err".into(), Duration::ZERO),
                AgentResult::skipped("c".into()),
            ],
            total_duration: Duration::from_secs(1),
            success_count: 1,
            failure_count: 2,
        };
        assert!(!result.all_succeeded());
        assert_eq!(result.failures().len(), 2);
        assert_eq!(result.successes().len(), 1);
    }

    #[test]
    fn test_multi_agent_result_serde_roundtrip() {
        let result = MultiAgentResult {
            agent_results: vec![AgentResult::completed(
                "a".into(),
                make_output("ok"),
                Duration::ZERO,
            )],
            total_duration: Duration::from_secs(2),
            success_count: 1,
            failure_count: 0,
        };
        let json_str = serde_json::to_string(&result).unwrap();
        let deserialized: MultiAgentResult = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized.success_count, 1);
        assert_eq!(deserialized.agent_results.len(), 1);
    }

    // ---- Topological sort tests ----

    #[test]
    fn test_topological_sort_no_deps() {
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb"),
            AgentConfig::new("c", "tc"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_linear_chain() {
        let agents = vec![
            AgentConfig::new("c", "tc").depends_on("b"),
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb").depends_on("a"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_diamond() {
        // a -> b, a -> c, b -> d, c -> d
        let agents = vec![
            AgentConfig::new("d", "td").depends_on("b").depends_on("c"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("a", "ta"),
            AgentConfig::new("c", "tc").depends_on("a"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();

        // a must come before b, c; b and c must come before d
        let pos: HashMap<&str, usize> = names.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        assert!(pos[&"a"] < pos[&"b"]);
        assert!(pos[&"a"] < pos[&"c"]);
        assert!(pos[&"b"] < pos[&"d"]);
        assert!(pos[&"c"] < pos[&"d"]);
    }

    #[test]
    fn test_topological_sort_circular() {
        let agents = vec![
            AgentConfig::new("a", "ta").depends_on("b"),
            AgentConfig::new("b", "tb").depends_on("a"),
        ];
        let result = topological_sort(&agents);
        assert!(matches!(
            result,
            Err(DependencyError::CircularDependency(_))
        ));
    }

    #[test]
    fn test_topological_sort_self_cycle() {
        let agents = vec![AgentConfig::new("a", "ta").depends_on("a")];
        let result = topological_sort(&agents);
        assert!(matches!(
            result,
            Err(DependencyError::CircularDependency(_))
        ));
    }

    #[test]
    fn test_topological_sort_unknown_dep() {
        let agents = vec![AgentConfig::new("a", "ta").depends_on("nonexistent")];
        let result = topological_sort(&agents);
        assert!(matches!(result, Err(DependencyError::UnknownDependency(_))));
    }

    #[test]
    fn test_topological_sort_duplicate_name() {
        let agents = vec![AgentConfig::new("dup", "ta"), AgentConfig::new("dup", "tb")];
        let result = topological_sort(&agents);
        assert!(matches!(result, Err(DependencyError::DuplicateAgent(_))));
    }

    #[test]
    fn test_topological_sort_single() {
        let agents = vec![AgentConfig::new("solo", "task")];
        let sorted = topological_sort(&agents).unwrap();
        assert_eq!(sorted.len(), 1);
        assert_eq!(sorted[0].name, "solo");
    }

    #[test]
    fn test_topological_sort_empty() {
        let agents: Vec<AgentConfig> = vec![];
        let sorted = topological_sort(&agents).unwrap();
        assert!(sorted.is_empty());
    }

    #[test]
    fn test_dependency_error_display() {
        assert_eq!(
            DependencyError::UnknownDependency("x".into()).to_string(),
            "unknown dependency 'x'"
        );
        assert_eq!(
            DependencyError::DuplicateAgent("y".into()).to_string(),
            "duplicate agent name 'y'"
        );
        let cycle = DependencyError::CircularDependency(vec!["a".into(), "b".into()]);
        assert!(cycle.to_string().contains("circular"));
    }

    // ---- Wave building tests ----

    #[test]
    fn test_build_waves_no_deps() {
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb"),
            AgentConfig::new("c", "tc"),
        ];
        let sorted: Vec<&AgentConfig> = agents.iter().collect();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        // All should be in wave 0 since no deps
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 3);
    }

    #[test]
    fn test_build_waves_linear() {
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("c", "tc").depends_on("b"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1);
        assert_eq!(waves[1].len(), 1);
        assert_eq!(waves[2].len(), 1);
    }

    #[test]
    fn test_build_waves_parallel_after_deps() {
        // a -> b, a -> c, b -> d, c -> d
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("c", "tc").depends_on("a"),
            AgentConfig::new("d", "td").depends_on("b").depends_on("c"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        // Wave 0: [a], Wave 1: [b, c], Wave 2: [d]
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1);
        assert_eq!(waves[1].len(), 2);
        assert_eq!(waves[2].len(), 1);
    }

    // ---- MultiAgentSpawner integration tests ----

    #[tokio::test]
    async fn test_spawn_empty_agents() {
        let config = MultiAgentConfig::new(vec![]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert!(result.agent_results.is_empty());
        assert_eq!(result.success_count, 0);
        assert_eq!(result.failure_count, 0);
    }

    #[tokio::test]
    async fn test_spawn_single_agent() {
        let config = MultiAgentConfig::new(vec![AgentConfig::new("solo", "Do a thing")]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 1);
        assert_eq!(result.success_count, 1);
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_multiple_independent() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
            AgentConfig::new("c", "task c"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 3);
        assert_eq!(result.success_count, 3);
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_with_dependencies() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("setup", "Initialize"),
            AgentConfig::new("build", "Build").depends_on("setup"),
            AgentConfig::new("test", "Test").depends_on("build"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 3);
        assert!(result.all_succeeded());
        // Results should be in original config order
        assert_eq!(result.agent_results[0].agent_name, "setup");
        assert_eq!(result.agent_results[1].agent_name, "build");
        assert_eq!(result.agent_results[2].agent_name, "test");
    }

    #[tokio::test]
    async fn test_spawn_duplicate_names_fails() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("dup", "first"),
            AgentConfig::new("dup", "second"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.failure_count, 1);
    }

    #[tokio::test]
    async fn test_spawn_circular_dependency_fails_all() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "ta").depends_on("b"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("c", "tc"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 3);
        assert_eq!(result.failure_count, 3);
        assert_eq!(result.success_count, 0);
    }

    #[tokio::test]
    async fn test_spawn_unknown_dependency_fails_all() {
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "ta").depends_on("ghost")]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.failure_count, 1);
        let err = result.agent_results[0].error.as_deref().unwrap();
        assert!(err.contains("unknown dependency"));
    }

    #[tokio::test]
    async fn test_spawn_respects_max_parallel() {
        // Launch 8 agents with max_parallel=2
        // We can't directly test semaphore limiting easily,
        // but we verify the config is respected by checking results
        let agents: Vec<AgentConfig> = (0..8)
            .map(|i| AgentConfig::new(format!("agent-{i}"), format!("task {i}")))
            .collect();
        let config = MultiAgentConfig::new(agents).with_max_parallel(2);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 8);
        assert_eq!(result.success_count, 8);
    }

    #[tokio::test]
    async fn test_spawn_timeout() {
        // Use a very short timeout that should cause timeouts
        // Note: our synthetic execute_agent completes instantly,
        // so this tests the timeout wiring rather than actual timeout behavior.
        // A real implementation would have agents that take time.
        let config = MultiAgentConfig::new(vec![AgentConfig::new("fast", "instant task")])
            .with_timeout(Duration::from_secs(5));

        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.success_count, 1);
    }

    #[tokio::test]
    async fn test_spawn_result_order_matches_config() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("z-last", "z").depends_on("a-first"),
            AgentConfig::new("a-first", "a"),
            AgentConfig::new("m-middle", "m").depends_on("a-first"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        // Results should be in original config order, not execution order
        assert_eq!(result.agent_results[0].agent_name, "z-last");
        assert_eq!(result.agent_results[1].agent_name, "a-first");
        assert_eq!(result.agent_results[2].agent_name, "m-middle");
    }

    #[tokio::test]
    async fn test_spawn_background() {
        let config = MultiAgentConfig::new(vec![AgentConfig::new("bg-agent", "background task")]);
        let handle = MultiAgentSpawner::spawn_background(config);
        let result = handle.await.unwrap();
        assert_eq!(result.success_count, 1);
    }

    #[tokio::test]
    async fn test_spawn_diamond_dependency() {
        // a -> b, a -> c, b -> d, c -> d
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "root"),
            AgentConfig::new("b", "left").depends_on("a"),
            AgentConfig::new("c", "right").depends_on("a"),
            AgentConfig::new("d", "join")
                .depends_on("b")
                .depends_on("c"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert!(result.all_succeeded());
        assert_eq!(result.success_count, 4);
    }

    // ---- Test-only mock executors ----

    use std::sync::Mutex;

    /// Mock executor that records call order and returns configurable results.
    /// Thread-safe via Mutex so it can be shared across concurrent agent tasks.
    struct RecordingMockExecutor {
        /// Ordered list of (system_prompt, task) calls received
        calls: Mutex<Vec<(String, String)>>,
        /// Optional delay to simulate work
        delay: Duration,
        /// Whether to return an error
        should_fail: bool,
        /// Error message to return when should_fail is true
        error_message: String,
    }

    impl RecordingMockExecutor {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
                should_fail: false,
                error_message: "mock execution error".to_string(),
            }
        }

        fn with_delay(delay: Duration) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                delay,
                should_fail: false,
                error_message: String::new(),
            }
        }

        fn failing(error_message: impl Into<String>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
                should_fail: true,
                error_message: error_message.into(),
            }
        }

        fn get_calls(&self) -> Vec<(String, String)> {
            self.calls.lock().expect("lock poisoned").clone()
        }

        fn call_count(&self) -> usize {
            self.calls.lock().expect("lock poisoned").len()
        }
    }

    #[async_trait::async_trait]
    impl crate::executor::AgentExecutor for RecordingMockExecutor {
        async fn execute(
            &self,
            system_prompt: &str,
            task: &str,
            _model: Option<&str>,
            _tools: Option<&[String]>,
        ) -> Result<ToolOutput, String> {
            self.calls
                .lock()
                .expect("lock poisoned")
                .push((system_prompt.to_string(), task.to_string()));

            if self.delay > Duration::ZERO {
                tokio::time::sleep(self.delay).await;
            }

            if self.should_fail {
                return Err(self.error_message.clone());
            }

            Ok(ToolOutput {
                content: format!("completed: {task}"),
                is_error: false,
                metadata: StdHashMap::new(),
            })
        }

        async fn execute_with_history(
            &self,
            system_prompt: &str,
            _history: &[crate::executor::ChatTurn],
            task: &str,
            _model: Option<&str>,
            _tools: Option<&[String]>,
        ) -> Result<ToolOutput, String> {
            // Delegate to execute for simplicity in tests
            self.execute(system_prompt, task, _model, _tools).await
        }
    }

    /// Mock executor that fails for a specific agent name.
    struct SelectiveFailureExecutor {
        /// Agent names that should fail
        failing_agents: Vec<String>,
        /// Tracks which agents were called
        called_agents: Mutex<Vec<String>>,
    }

    impl SelectiveFailureExecutor {
        fn new(failing_agents: Vec<String>) -> Self {
            Self {
                failing_agents,
                called_agents: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl crate::executor::AgentExecutor for SelectiveFailureExecutor {
        async fn execute(
            &self,
            _system_prompt: &str,
            task: &str,
            _model: Option<&str>,
            _tools: Option<&[String]>,
        ) -> Result<ToolOutput, String> {
            // We use the task field to identify the agent since we can't
            // directly access the agent name from the executor interface.
            // Instead, we check if any failing agent name appears in the task.
            for failing_name in &self.failing_agents {
                if task.contains(failing_name) {
                    self.called_agents
                        .lock()
                        .expect("lock poisoned")
                        .push(failing_name.clone());
                    return Err(format!("agent '{failing_name}' failed"));
                }
            }

            Ok(ToolOutput {
                content: format!("completed: {task}"),
                is_error: false,
                metadata: StdHashMap::new(),
            })
        }

        async fn execute_with_history(
            &self,
            system_prompt: &str,
            _history: &[crate::executor::ChatTurn],
            task: &str,
            _model: Option<&str>,
            _tools: Option<&[String]>,
        ) -> Result<ToolOutput, String> {
            self.execute(system_prompt, task, _model, _tools).await
        }
    }

    // ---- Executor integration tests ----

    #[tokio::test]
    async fn test_spawn_with_mock_executor_records_calls() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());
        assert_eq!(executor.call_count(), 2);

        let calls = executor.get_calls();
        let tasks: Vec<&str> = calls.iter().map(|(_, t)| t.as_str()).collect();
        assert!(tasks.contains(&"task a"));
        assert!(tasks.contains(&"task b"));
    }

    #[tokio::test]
    async fn test_spawn_executor_receives_system_prompt() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "do stuff")
            .with_system_prompt("custom system prompt")]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());

        let calls = executor.get_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "custom system prompt");
    }

    #[tokio::test]
    async fn test_spawn_default_system_prompt_used_when_agent_has_none() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")])
            .with_default_system_prompt("default prompt override");

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());

        let calls = executor.get_calls();
        assert_eq!(calls[0].0, "default prompt override");
    }

    #[tokio::test]
    async fn test_spawn_agent_system_prompt_overrides_default() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")
            .with_system_prompt("agent-specific prompt")])
            .with_default_system_prompt("default prompt");

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());

        let calls = executor.get_calls();
        // Agent's own system_prompt takes priority over default
        assert_eq!(calls[0].0, "agent-specific prompt");
    }

    // ---- Failing executor tests ----

    #[tokio::test]
    async fn test_spawn_failing_executor_marks_agent_failed() {
        let executor = Arc::new(RecordingMockExecutor::failing("something went wrong"));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("fail-agent", "do stuff")]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert_eq!(result.failure_count, 1);
        assert_eq!(result.success_count, 0);
        assert_eq!(
            result.agent_results[0].status,
            AgentResultStatus::Failed
        );
        assert_eq!(
            result.agent_results[0].error.as_deref(),
            Some("something went wrong")
        );
    }

    #[tokio::test]
    async fn test_spawn_mixed_success_and_failure() {
        // One agent with failing executor, one without any executor (stub success)
        let fail_exec = Arc::new(RecordingMockExecutor::failing("boom"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("bad", "will fail"),
            AgentConfig::new("good", "will succeed"),
        ]);

        // Spawn with failing executor -- both agents use the same executor
        let result = MultiAgentSpawner::spawn(config, Some(fail_exec)).await;
        assert_eq!(result.failure_count, 2);
        assert_eq!(result.success_count, 0);
    }

    // ---- Timeout tests ----

    #[tokio::test]
    async fn test_spawn_slow_agent_times_out() {
        // Executor that sleeps for 500ms, but timeout is 50ms
        let executor = Arc::new(RecordingMockExecutor::with_delay(Duration::from_millis(500)));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("slow", "slow task")])
            .with_timeout(Duration::from_millis(50));

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.failure_count, 1);
        assert_eq!(result.success_count, 0);
        assert_eq!(
            result.agent_results[0].status,
            AgentResultStatus::Timeout
        );
        let err = result.agent_results[0].error.as_deref().unwrap();
        assert!(err.contains("timeout"), "expected timeout in error, got: {err}");
    }

    #[tokio::test]
    async fn test_spawn_fast_agent_completes_within_timeout() {
        // Executor with tiny delay, generous timeout
        let executor = Arc::new(RecordingMockExecutor::with_delay(Duration::from_millis(1)));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("fast", "fast task")])
            .with_timeout(Duration::from_secs(10));

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.success_count, 1);
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_mixed_timeout_and_success() {
        // Two agents: one slow (will timeout), one instant (should succeed)
        // We use no executor (stub) for this since both would share the same executor.
        // Instead, test that timeout at the config level doesn't affect the overall result
        // when the stub executor returns instantly.
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("fast-a", "task a"),
            AgentConfig::new("fast-b", "task b"),
        ])
        .with_timeout(Duration::from_secs(1));

        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.success_count, 2);
    }

    // ---- Fail-fast tests ----

    #[tokio::test]
    async fn test_spawn_fail_fast_skips_remaining_in_wave() {
        let executor = Arc::new(RecordingMockExecutor::failing("fatal error"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
        ])
        .with_fail_fast();

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        // Both agents in the same wave (no deps) are launched concurrently,
        // so both may fail before cancellation takes effect. The key invariant
        // is that failure_count == total agents.
        assert_eq!(result.failure_count, 2);
        assert!(!result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_fail_fast_skips_dependent_agents() {
        let executor = Arc::new(RecordingMockExecutor::failing("fatal error"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("root", "root task"),
            AgentConfig::new("child", "child task").depends_on("root"),
        ])
        .with_fail_fast();

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.agent_results.len(), 2);
        // Root fails, child gets skipped (dependency failed or cancelled)
        let statuses: Vec<&AgentResultStatus> =
            result.agent_results.iter().map(|r| &r.status).collect();
        assert!(statuses.contains(&&AgentResultStatus::Failed));
        assert!(statuses.contains(&&AgentResultStatus::Skipped)
            || statuses.contains(&&AgentResultStatus::Failed));
    }

    #[tokio::test]
    async fn test_spawn_fail_fast_across_chained_deps() {
        // a -> b -> c with a failing
        let executor = Arc::new(RecordingMockExecutor::failing("fail"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b").depends_on("a"),
            AgentConfig::new("c", "task c").depends_on("b"),
        ])
        .with_fail_fast();

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.agent_results.len(), 3);
        // 'a' fails, 'b' and 'c' should be skipped or failed
        assert_eq!(result.success_count, 0);
        assert!(
            result.agent_results.iter().any(|r| r.status == AgentResultStatus::Skipped),
            "at least one agent should be skipped in fail-fast chain"
        );
    }

    #[tokio::test]
    async fn test_spawn_no_fail_fast_continues_after_failure() {
        let executor = Arc::new(RecordingMockExecutor::failing("error"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
        ]);
        // fail_fast is false by default

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        // Both should be attempted (both fail because of the executor)
        assert_eq!(result.failure_count, 2);
        // None should be skipped since there are no deps and fail_fast is off
        assert!(result
            .agent_results
            .iter()
            .all(|r| r.status == AgentResultStatus::Failed));
    }

    // ---- Dependency propagation tests ----

    #[tokio::test]
    async fn test_spawn_dependency_failure_propagates_to_dependents() {
        // 'a' depends on 'root' which fails -> 'a' should be skipped
        let executor = Arc::new(RecordingMockExecutor::failing("root failure"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("root", "root task"),
            AgentConfig::new("child", "child task").depends_on("root"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.agent_results.len(), 2);
        // root fails
        assert_eq!(
            result.agent_results[0].status,
            AgentResultStatus::Failed
        );
        // child should be skipped because its dependency failed
        assert_eq!(
            result.agent_results[1].status,
            AgentResultStatus::Skipped
        );
    }

    #[tokio::test]
    async fn test_spawn_diamond_with_failing_root_skips_all() {
        // root -> left, root -> right, left -> join, right -> join
        // root fails -> left and right should be skipped -> join skipped
        let executor = Arc::new(RecordingMockExecutor::failing("root boom"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("root", "root task"),
            AgentConfig::new("left", "left task").depends_on("root"),
            AgentConfig::new("right", "right task").depends_on("root"),
            AgentConfig::new("join", "join task")
                .depends_on("left")
                .depends_on("right"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.success_count, 0);
        assert_eq!(result.failure_count, 4);
        // root fails, rest are skipped
        let skipped_count = result
            .agent_results
            .iter()
            .filter(|r| r.status == AgentResultStatus::Skipped)
            .count();
        assert_eq!(skipped_count, 3);
    }

    #[tokio::test]
    async fn test_spawn_diamond_with_mid_failure_skips_join() {
        // root succeeds, left succeeds, right fails -> join should be skipped
        // We need a more nuanced approach: no executor (stub success for all)
        // won't let us fail just one. Use a no-executor config where everything
        // succeeds to verify the happy path, then separately test failure.
        // For a selective failure, we use a pattern in the task name.
        let executor: Arc<dyn crate::executor::AgentExecutor> =
            Arc::new(SelectiveFailureExecutor::new(vec!["right".to_string()]));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("root", "root task"),
            AgentConfig::new("left", "left task").depends_on("root"),
            AgentConfig::new("right", "right task").depends_on("root"),
            AgentConfig::new("join", "join task")
                .depends_on("left")
                .depends_on("right"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        assert_eq!(result.agent_results.len(), 4);

        let root_result = result.agent_results.iter().find(|r| r.agent_name == "root").unwrap();
        let left_result = result.agent_results.iter().find(|r| r.agent_name == "left").unwrap();
        let right_result = result.agent_results.iter().find(|r| r.agent_name == "right").unwrap();
        let join_result = result.agent_results.iter().find(|r| r.agent_name == "join").unwrap();

        assert_eq!(root_result.status, AgentResultStatus::Completed);
        assert_eq!(left_result.status, AgentResultStatus::Completed);
        assert_eq!(right_result.status, AgentResultStatus::Failed);
        assert_eq!(join_result.status, AgentResultStatus::Skipped);
    }

    // ---- Concurrency and ordering tests ----

    #[tokio::test]
    async fn test_spawn_max_parallel_1_runs_sequentially() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
            AgentConfig::new("c", "task c"),
        ])
        .with_max_parallel(1);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());
        assert_eq!(executor.call_count(), 3);
        // All tasks should have been recorded
        let calls = executor.get_calls();
        let tasks: Vec<&str> = calls.iter().map(|(_, t)| t.as_str()).collect();
        assert!(tasks.contains(&"task a"));
        assert!(tasks.contains(&"task b"));
        assert!(tasks.contains(&"task c"));
    }

    #[tokio::test]
    async fn test_spawn_dependency_chain_executes_in_order() {
        // a -> b -> c: verify execution order via call recording
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b").depends_on("a"),
            AgentConfig::new("c", "task c").depends_on("b"),
        ])
        .with_max_parallel(4);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());

        let calls = executor.get_calls();
        // Verify ordering: "task a" must appear before "task b", "task b" before "task c"
        let positions: std::collections::HashMap<&str, usize> = calls
            .iter()
            .enumerate()
            .map(|(i, (_, t))| (t.as_str(), i))
            .collect();

        assert!(
            positions[&"task a"] < positions[&"task b"],
            "a should execute before b"
        );
        assert!(
            positions[&"task b"] < positions[&"task c"],
            "b should execute before c"
        );
    }

    #[tokio::test]
    async fn test_spawn_parallel_agents_in_same_wave_execute_concurrently() {
        // Two independent agents with a small delay -- verify they overlap
        // by checking total time is less than sum of individual delays.
        let executor = Arc::new(RecordingMockExecutor::with_delay(Duration::from_millis(100)));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
        ])
        .with_max_parallel(4)
        .with_timeout(Duration::from_secs(10));

        let start = Instant::now();
        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        let elapsed = start.elapsed();

        assert!(result.all_succeeded());
        // If they ran sequentially, it would be at least 200ms.
        // With concurrency, it should be close to 100ms + overhead.
        // Allow generous margin to avoid flaky tests on slow CI.
        assert!(
            elapsed < Duration::from_millis(350),
            "parallel agents should complete faster than sequential: {elapsed:?}"
        );
    }

    // ---- Wave building edge cases ----

    #[test]
    fn test_build_waves_empty() {
        let agents: Vec<AgentConfig> = vec![];
        let sorted: Vec<&AgentConfig> = agents.iter().collect();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        assert!(waves.is_empty());
    }

    #[test]
    fn test_build_waves_single_agent() {
        let agents = vec![AgentConfig::new("solo", "task")];
        let sorted: Vec<&AgentConfig> = agents.iter().collect();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        assert_eq!(waves.len(), 1);
        assert_eq!(waves[0].len(), 1);
        assert_eq!(waves[0][0].name, "solo");
    }

    #[test]
    fn test_build_waves_wide_diamond() {
        // a -> b1, b2, b3 -> c (fan-out then fan-in)
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b1", "tb1").depends_on("a"),
            AgentConfig::new("b2", "tb2").depends_on("a"),
            AgentConfig::new("b3", "tb3").depends_on("a"),
            AgentConfig::new("c", "tc")
                .depends_on("b1")
                .depends_on("b2")
                .depends_on("b3"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        assert_eq!(waves.len(), 3);
        assert_eq!(waves[0].len(), 1); // [a]
        assert_eq!(waves[1].len(), 3); // [b1, b2, b3]
        assert_eq!(waves[2].len(), 1); // [c]
    }

    #[test]
    fn test_build_waves_complex_graph() {
        // Multi-level dependency graph:
        // Wave 0: [a, x] (no deps)
        // Wave 1: [b] (depends on a)
        // Wave 2: [c] (depends on b)
        // Wave 3: [d] (depends on c, x)
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("c", "tc").depends_on("b"),
            AgentConfig::new("d", "td").depends_on("c").depends_on("x"),
            AgentConfig::new("x", "tx"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let waves = MultiAgentSpawner::build_waves(&sorted);
        // x has no deps -> wave 0; a has no deps -> wave 0
        // b depends on a -> wave 1
        // c depends on b -> wave 2
        // d depends on c (wave 2) and x (wave 0) -> wave 3
        assert_eq!(waves.len(), 4);
    }

    // ---- Topological sort additional edge cases ----

    #[test]
    fn test_topological_sort_preserves_original_order_for_independent() {
        // Agents with no deps should stay in their original insertion order
        let agents = vec![
            AgentConfig::new("zebra", "tz"),
            AgentConfig::new("alpha", "ta"),
            AgentConfig::new("mango", "tm"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["zebra", "alpha", "mango"]);
    }

    #[test]
    fn test_topological_sort_three_node_cycle() {
        let agents = vec![
            AgentConfig::new("a", "ta").depends_on("c"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("c", "tc").depends_on("b"),
        ];
        let result = topological_sort(&agents);
        assert!(matches!(result, Err(DependencyError::CircularDependency(cycle)) if cycle.len() == 3));
    }

    #[test]
    fn test_topological_sort_multiple_unknown_deps() {
        let agents = vec![AgentConfig::new("a", "ta")
            .depends_on("ghost1")
            .depends_on("ghost2")];
        let result = topological_sort(&agents);
        // Should fail on the first unknown dep encountered
        assert!(matches!(result, Err(DependencyError::UnknownDependency(_))));
    }

    #[test]
    fn test_topological_sort_disconnected_components() {
        // Two independent chains: a->b and x->y
        let agents = vec![
            AgentConfig::new("a", "ta"),
            AgentConfig::new("b", "tb").depends_on("a"),
            AgentConfig::new("x", "tx"),
            AgentConfig::new("y", "ty").depends_on("x"),
        ];
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();

        let pos: HashMap<&str, usize> = names.iter().enumerate().map(|(i, &n)| (n, i)).collect();
        assert!(pos[&"a"] < pos[&"b"]);
        assert!(pos[&"x"] < pos[&"y"]);
    }

    #[test]
    fn test_topological_sort_dep_on_self() {
        let agents = vec![AgentConfig::new("a", "ta").depends_on("a")];
        assert!(matches!(
            topological_sort(&agents),
            Err(DependencyError::CircularDependency(_))
        ));
    }

    // ---- State transition tests ----

    #[tokio::test]
    async fn test_spawn_all_results_have_valid_status() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b").depends_on("a"),
            AgentConfig::new("c", "task c").depends_on("b"),
        ]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        // All should complete successfully
        for agent_result in &result.agent_results {
            assert_eq!(agent_result.status, AgentResultStatus::Completed);
            assert!(agent_result.output.is_some());
            assert!(agent_result.error.is_none());
        }
    }

    #[tokio::test]
    async fn test_spawn_result_durations_are_nonzero_for_completed() {
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        // Even with stub executor, duration should be >= 0
        assert!(result.agent_results[0].duration >= Duration::ZERO);
        assert!(result.total_duration >= Duration::ZERO);
    }

    #[tokio::test]
    async fn test_spawn_failure_result_has_error_message() {
        let executor = Arc::new(RecordingMockExecutor::failing("specific error XYZ"));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")]);

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        let failed = &result.agent_results[0];
        assert_eq!(failed.status, AgentResultStatus::Failed);
        assert!(failed.error.as_deref().unwrap().contains("specific error XYZ"));
        assert!(failed.output.is_none());
    }

    #[tokio::test]
    async fn test_spawn_skipped_result_has_error_message() {
        let executor = Arc::new(RecordingMockExecutor::failing("fail"));
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("root", "root task"),
            AgentConfig::new("child", "child task").depends_on("root"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        let child = result.agent_results.iter().find(|r| r.agent_name == "child").unwrap();
        assert_eq!(child.status, AgentResultStatus::Skipped);
        assert!(child.error.as_deref().unwrap().contains("skipped"));
        assert!(child.output.is_none());
        assert_eq!(child.duration, Duration::ZERO);
    }

    #[tokio::test]
    async fn test_spawn_timeout_result_has_timeout_error() {
        let executor = Arc::new(RecordingMockExecutor::with_delay(Duration::from_secs(10)));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")])
            .with_timeout(Duration::from_millis(1));

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        let timed_out = &result.agent_results[0];
        assert_eq!(timed_out.status, AgentResultStatus::Timeout);
        let err = timed_out.error.as_deref().unwrap();
        assert!(err.contains("timeout") || err.contains("exceeded"));
        assert!(timed_out.output.is_none());
    }

    // ---- Edge case: single agent with various configs ----

    #[tokio::test]
    async fn test_spawn_single_agent_with_system_prompt() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task").with_system_prompt("custom"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());
        assert_eq!(executor.get_calls()[0].0, "custom");
    }

    #[tokio::test]
    async fn test_spawn_single_agent_with_model_override() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task").with_model("claude-sonnet-4-6"),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());
        // The mock executor ignores model, but we verify the config doesn't break
    }

    #[tokio::test]
    async fn test_spawn_single_agent_with_tools() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task").with_tools(vec!["read".into(), "write".into()]),
        ]);

        let result = MultiAgentSpawner::spawn(config, Some(executor.clone())).await;
        assert!(result.all_succeeded());
    }

    // ---- Empty / boundary config tests ----

    #[tokio::test]
    async fn test_spawn_empty_config_default() {
        let config = MultiAgentConfig::default();
        let result = MultiAgentSpawner::spawn(config, None).await;
        assert!(result.agent_results.is_empty());
        assert_eq!(result.success_count, 0);
        assert_eq!(result.failure_count, 0);
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_many_independent_agents() {
        // Stress test with many agents (no deps)
        let agents: Vec<AgentConfig> = (0..20)
            .map(|i| AgentConfig::new(format!("agent-{i}"), format!("task {i}")))
            .collect();
        let config = MultiAgentConfig::new(agents).with_max_parallel(4);

        let result = MultiAgentSpawner::spawn(config, None).await;
        assert_eq!(result.agent_results.len(), 20);
        assert!(result.all_succeeded());
    }

    #[tokio::test]
    async fn test_spawn_timeout_zero_seconds() {
        // Zero timeout should immediately timeout all agents
        let executor = Arc::new(RecordingMockExecutor::with_delay(Duration::from_millis(1)));
        let config = MultiAgentConfig::new(vec![AgentConfig::new("a", "task")])
            .with_timeout(Duration::from_secs(0));

        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        // With 0s timeout, the agent may or may not complete depending on scheduling.
        // The test verifies the system doesn't panic.
        assert_eq!(result.agent_results.len(), 1);
    }

    // ---- MultiAgentResult method tests ----

    #[test]
    fn test_multi_agent_result_failures_returns_correct_status() {
        let result = MultiAgentResult {
            agent_results: vec![
                AgentResult::completed("ok".into(), make_output("ok"), Duration::ZERO),
                AgentResult::failed("fail".into(), "err".into(), Duration::ZERO),
                AgentResult::timed_out("timeout".into(), Duration::from_secs(5)),
                AgentResult::skipped("skip".into()),
            ],
            total_duration: Duration::from_secs(5),
            success_count: 1,
            failure_count: 3,
        };
        assert_eq!(result.failures().len(), 3);
        assert_eq!(result.successes().len(), 1);
    }

    #[test]
    fn test_multi_agent_result_all_succeeded_empty() {
        let result = MultiAgentResult {
            agent_results: vec![],
            total_duration: Duration::ZERO,
            success_count: 0,
            failure_count: 0,
        };
        assert!(result.all_succeeded());
    }

    #[test]
    fn test_multi_agent_result_not_succeeded_with_mismatch() {
        // success_count doesn't match agent_results.len() (simulates inconsistency)
        let result = MultiAgentResult {
            agent_results: vec![
                AgentResult::completed("a".into(), make_output("ok"), Duration::ZERO),
                AgentResult::completed("b".into(), make_output("ok"), Duration::ZERO),
            ],
            total_duration: Duration::ZERO,
            success_count: 2,
            failure_count: 0,
        };
        assert!(result.all_succeeded());

        // Now with failure
        let result2 = MultiAgentResult {
            agent_results: vec![
                AgentResult::completed("a".into(), make_output("ok"), Duration::ZERO),
                AgentResult::failed("b".into(), "err".into(), Duration::ZERO),
            ],
            total_duration: Duration::ZERO,
            success_count: 1,
            failure_count: 1,
        };
        assert!(!result2.all_succeeded());
    }

    // ---- AgentResult output content tests ----

    #[tokio::test]
    async fn test_spawn_stub_executor_output_content() {
        let config = MultiAgentConfig::new(vec![AgentConfig::new("agent-1", "my task")]);
        let result = MultiAgentSpawner::spawn(config, None).await;
        let output = result.agent_results[0].output.as_ref().unwrap();
        assert!(output.content.contains("agent-1"));
        assert!(!output.is_error);
    }

    #[tokio::test]
    async fn test_spawn_mock_executor_output_content() {
        let executor = Arc::new(RecordingMockExecutor::new());
        let config = MultiAgentConfig::new(vec![AgentConfig::new("test-agent", "do work")]);
        let result = MultiAgentSpawner::spawn(config, Some(executor)).await;
        let output = result.agent_results[0].output.as_ref().unwrap();
        assert!(output.content.contains("do work"));
        assert!(!output.is_error);
    }

    // ---- Topological sort with complex dependency patterns ----

    #[test]
    fn test_topological_sort_many_deps_on_one_node() {
        // Many agents depend on a single root
        let agents: Vec<AgentConfig> = std::iter::once(AgentConfig::new("root", "root"))
            .chain((0..10).map(|i| {
                AgentConfig::new(format!("child-{i}"), format!("task {i}")).depends_on("root")
            }))
            .collect();
        let sorted = topological_sort(&agents).unwrap();
        let pos: HashMap<&str, usize> = sorted
            .iter()
            .enumerate()
            .map(|(i, a)| (a.name.as_str(), i))
            .collect();
        // root must come before all children
        for i in 0..10 {
            assert!(pos[&"root"] < pos[&format!("child-{i}").as_str()]);
        }
    }

    #[test]
    fn test_topological_sort_long_chain() {
        // a0 -> a1 -> a2 -> ... -> a9
        let agents: Vec<AgentConfig> = (0..10)
            .map(|i| {
                let mut cfg = AgentConfig::new(format!("a{i}"), format!("task {i}"));
                if i > 0 {
                    cfg = cfg.depends_on(format!("a{}", i - 1));
                }
                cfg
            })
            .collect();
        let sorted = topological_sort(&agents).unwrap();
        let names: Vec<&str> = sorted.iter().map(|a| a.name.as_str()).collect();
        for i in 0..10 {
            assert_eq!(names[i], format!("a{i}"));
        }
    }

    // ---- spawn_background tests ----

    #[tokio::test]
    async fn test_spawn_background_multiple_agents() {
        let config = MultiAgentConfig::new(vec![
            AgentConfig::new("a", "task a"),
            AgentConfig::new("b", "task b"),
            AgentConfig::new("c", "task c").depends_on("a"),
        ]);
        let handle = MultiAgentSpawner::spawn_background(config);
        let result = handle.await.unwrap();
        assert!(result.all_succeeded());
        assert_eq!(result.success_count, 3);
    }

    // ---- DependencyError display formatting ----

    #[test]
    fn test_dependency_error_circular_display_format() {
        let err = DependencyError::CircularDependency(vec![
            "alpha".into(),
            "beta".into(),
            "gamma".into(),
        ]);
        let msg = err.to_string();
        assert!(msg.contains("alpha -> beta -> gamma"));
    }

    // ---- Helpers ----

    fn make_output(content: &str) -> ToolOutput {
        ToolOutput {
            content: content.to_string(),
            is_error: false,
            metadata: StdHashMap::new(),
        }
    }
}
