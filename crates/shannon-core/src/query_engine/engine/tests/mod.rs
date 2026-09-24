//! `QueryEngine` unit tests. Moved verbatim from the former single-file
//! `engine.rs` `#[cfg(test)] mod tests` and grouped thematically; shared
//! helpers live here.

use super::agent_loop::{ToolResultEntry, WRAP_UP_NUDGE_PROMPT};
use super::events::{AbortOnDropStream, EventTx};
use super::*;
use crate::query_engine::QueryMetadata;
use crate::query_engine::recovery;
use crate::tools::ToolRegistry;
use shannon_engine::api::ImageSource;
use shannon_engine::api::{LlmClient, LlmClientConfig, MessageContent};
use shannon_engine::permissions::PermissionManager;
use std::env;
use std::fs;
use uuid::Uuid;

fn create_test_client() -> LlmClient {
    let config = LlmClientConfig {
        api_key: "test-key".to_string(),
        base_url: "http://localhost:11434".to_string(),
        model: "test-model".to_string(),
        provider: LlmProvider::Ollama,
        ..Default::default()
    };
    LlmClient::new(config)
}

fn create_test_engine() -> QueryEngine {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let state = StateManager::new();
    let config = QueryEngineConfig::default();
    QueryEngine::new(client, tools, permissions, state, config)
}

fn create_test_engine_with_state(state: StateManager) -> QueryEngine {
    let client = create_test_client();
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new();
    let config = QueryEngineConfig::default();
    QueryEngine::new(client, tools, permissions, state, config)
}

fn temp_dir_for_test(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir()
        .join("shannon-engine-test")
        .join(name)
        .join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// NOTE: children reference items through `use super::*` exactly as the
// original single `mod tests` did; no test body was edited.
mod agent_loop_tests;
mod engine_config_tests;
mod event_channel_tests;
mod parsing_tests;
mod prompt_config_tests;
mod session_tests;
mod tool_result_tests;
