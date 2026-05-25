//! Shared team context for agent coordination
//!
//! Provides a unified context object that bridges the main conversation loop
//! and agent tools, giving all team operations access to the coordinator,
//! agent registry, and LLM client configuration.

use crate::agent_defs::AgentDefinitionRegistry;
use crate::coordinator::{AgentCoordinator, CoordinatorConfig};
use crate::executor::AgentExecutor;
use crate::persistence::FilePersistence;
use crate::sub_agent::SubAgentRegistry;
use shannon_core::api::LlmClientConfig;
use std::sync::Arc;

/// Environment variable name for enabling agent teams feature.
pub const TEAMS_ENV_VAR: &str = "SHANNON_AGENT_TEAMS";

/// Check whether agent teams are enabled via environment variable.
///
/// Returns `true` when `SHANNON_AGENT_TEAMS` is set to any of:
/// `1`, `true`, `yes`, `on` (case-insensitive).
pub fn teams_enabled() -> bool {
    std::env::var(TEAMS_ENV_VAR)
        .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

/// Shared context injected into agent tools at session start.
///
/// Provides access to:
/// - `AgentCoordinator` for team management and message routing
/// - `SubAgentRegistry` for agent CRUD and messaging
/// - `LlmClientConfig` for creating sub-agent QueryEngines
/// - Optional `AgentExecutor` for giving Teammates real LLM capability
/// - Permission mode string inherited from the lead agent
#[derive(Clone)]
pub struct TeamContext {
    /// The coordinator managing all teams and message routing
    pub coordinator: Arc<AgentCoordinator>,
    /// Registry tracking all spawned agents and teams
    pub registry: Arc<SubAgentRegistry>,
    /// LLM client configuration for sub-agent execution
    pub client_config: LlmClientConfig,
    /// Optional shared executor for Teammate LLM calls
    pub executor: Option<Arc<dyn AgentExecutor>>,
    /// Lead agent's permission mode, inherited by spawned teammates.
    /// Examples: "default", "plan", "auto", "bypassPermissions".
    pub permission_mode: String,
    /// Custom agent definitions loaded from .shannon/agents/*.toml
    pub agent_definitions: AgentDefinitionRegistry,
}

impl std::fmt::Debug for TeamContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamContext")
            .field("permission_mode", &self.permission_mode)
            .field("has_executor", &self.executor.is_some())
            .field(
                "agent_definition_count",
                &self.agent_definitions.list_names().len(),
            )
            .finish()
    }
}

impl TeamContext {
    /// Create a new TeamContext with default coordinator configuration.
    ///
    /// Checks `SHANNON_AGENT_TEAMS` env var first — returns an error if disabled.
    /// Falls back to a default CoordinatorConfig if none provided.
    pub async fn new(client_config: LlmClientConfig) -> Result<Self, crate::error::AgentError> {
        if !teams_enabled() {
            return Err(crate::error::AgentError::Configuration(format!(
                "Agent teams disabled. Set {TEAMS_ENV_VAR}=1 to enable."
            )));
        }

        let coordinator_config = CoordinatorConfig::default();
        let mut coordinator = Arc::new(AgentCoordinator::new(coordinator_config).await?);

        // Initialize file persistence before creating registry (which clones Arc)
        if let Ok(persist) = FilePersistence::new() {
            if let Some(coord) = Arc::get_mut(&mut coordinator) {
                coord.set_persistence(persist);
            }
        }

        let registry = Arc::new(SubAgentRegistry::new(coordinator.clone()));
        if let Ok(count) = coordinator.load_from_disk().await {
            if count > 0 {
                tracing::info!(teams_loaded = count, "Loaded persisted teams from disk");
            }
        }

        // Load custom agent definitions from .shannon/agents/ and ~/.shannon/agents/
        let agent_definitions = AgentDefinitionRegistry::load_from_dirs();
        if !agent_definitions.is_empty() {
            tracing::info!(
                count = agent_definitions.list_names().len(),
                "Loaded custom agent definitions"
            );
        }

        Ok(Self {
            coordinator,
            registry,
            client_config,
            executor: None,
            permission_mode: "default".to_string(),
            agent_definitions,
        })
    }

    /// Create a TeamContext with a specific coordinator configuration.
    pub async fn with_config(
        client_config: LlmClientConfig,
        coordinator_config: CoordinatorConfig,
    ) -> Result<Self, crate::error::AgentError> {
        if !teams_enabled() {
            return Err(crate::error::AgentError::Configuration(format!(
                "Agent teams disabled. Set {TEAMS_ENV_VAR}=1 to enable."
            )));
        }

        let mut coordinator = Arc::new(AgentCoordinator::new(coordinator_config).await?);

        // Initialize file persistence before creating registry (which clones Arc)
        if let Ok(persist) = FilePersistence::new() {
            if let Some(coord) = Arc::get_mut(&mut coordinator) {
                coord.set_persistence(persist);
            }
        }

        let registry = Arc::new(SubAgentRegistry::new(coordinator.clone()));
        if let Ok(count) = coordinator.load_from_disk().await {
            if count > 0 {
                tracing::info!(teams_loaded = count, "Loaded persisted teams from disk");
            }
        }

        let agent_definitions = AgentDefinitionRegistry::load_from_dirs();

        Ok(Self {
            coordinator,
            registry,
            client_config,
            executor: None,
            permission_mode: "default".to_string(),
            agent_definitions,
        })
    }

    /// Set the shared executor for Teammate LLM calls.
    pub fn with_executor(mut self, executor: Arc<dyn AgentExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }

    /// Set the permission mode inherited from the lead agent.
    pub fn with_permission_mode(mut self, mode: impl Into<String>) -> Self {
        self.permission_mode = mode.into();
        self
    }

    /// Set custom agent definitions, overriding the auto-loaded ones.
    pub fn with_agent_definitions(mut self, defs: AgentDefinitionRegistry) -> Self {
        self.agent_definitions = defs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn teams_enabled_defaults_to_false() {
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
        assert!(!teams_enabled());
    }

    #[test]
    #[serial]
    fn teams_enabled_with_value_1() {
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "1");
        }
        assert!(teams_enabled());
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn teams_enabled_with_value_true() {
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "true");
        }
        assert!(teams_enabled());
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn teams_enabled_case_insensitive() {
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "YES");
        }
        assert!(teams_enabled());
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn teams_enabled_rejects_random_values() {
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "maybe");
        }
        assert!(!teams_enabled());
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
    }

    #[test]
    #[serial]
    fn new_returns_error_when_disabled() {
        // Set to "0" instead of remove_var to avoid race with parallel tests
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "0");
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = shannon_core::api::LlmClientConfig::default();
        let result = rt.block_on(TeamContext::new(config));
        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("SHANNON_AGENT_TEAMS"),
            "Error should mention env var: {err}"
        );
    }

    #[test]
    #[serial]
    fn permission_mode_builder() {
        unsafe {
            std::env::set_var(TEAMS_ENV_VAR, "1");
        }
        let rt = tokio::runtime::Runtime::new().unwrap();
        let config = shannon_core::api::LlmClientConfig::default();
        let ctx = match rt.block_on(TeamContext::new(config)) {
            Ok(ctx) => ctx,
            Err(_) => {
                // Coordinator may fail to bind in CI/coverage environments
                unsafe {
                    std::env::remove_var(TEAMS_ENV_VAR);
                }
                return;
            }
        };
        assert_eq!(ctx.permission_mode, "default");

        let ctx = ctx.with_permission_mode("auto");
        assert_eq!(ctx.permission_mode, "auto");

        unsafe {
            std::env::remove_var(TEAMS_ENV_VAR);
        }
    }
}
