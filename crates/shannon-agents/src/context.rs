//! Shared team context for agent coordination
//!
//! Provides a unified context object that bridges the main conversation loop
//! and agent tools, giving all team operations access to the coordinator,
//! agent registry, and LLM client configuration.

use crate::coordinator::{AgentCoordinator, CoordinatorConfig};
use crate::sub_agent::SubAgentRegistry;
use crate::executor::AgentExecutor;
use shannon_core::api::LlmClientConfig;
use std::sync::Arc;

/// Shared context injected into agent tools at session start.
///
/// Provides access to:
/// - `AgentCoordinator` for team management and message routing
/// - `SubAgentRegistry` for agent CRUD and messaging
/// - `LlmClientConfig` for creating sub-agent QueryEngines
/// - Optional `AgentExecutor` for giving Teammates real LLM capability
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
}

impl TeamContext {
    /// Create a new TeamContext with default coordinator configuration.
    ///
    /// Falls back to a default CoordinatorConfig if none provided.
    pub async fn new(client_config: LlmClientConfig) -> Result<Self, crate::error::AgentError> {
        let coordinator_config = CoordinatorConfig::default();
        let coordinator = Arc::new(AgentCoordinator::new(coordinator_config).await?);
        let registry = Arc::new(SubAgentRegistry::new(coordinator.clone()));

        Ok(Self {
            coordinator,
            registry,
            client_config,
            executor: None,
        })
    }

    /// Create a TeamContext with a specific coordinator configuration.
    pub async fn with_config(
        client_config: LlmClientConfig,
        coordinator_config: CoordinatorConfig,
    ) -> Result<Self, crate::error::AgentError> {
        let coordinator = Arc::new(AgentCoordinator::new(coordinator_config).await?);
        let registry = Arc::new(SubAgentRegistry::new(coordinator.clone()));

        Ok(Self {
            coordinator,
            registry,
            client_config,
            executor: None,
        })
    }

    /// Set the shared executor for Teammate LLM calls.
    pub fn with_executor(mut self, executor: Arc<dyn AgentExecutor>) -> Self {
        self.executor = Some(executor);
        self
    }
}
