//! Test environment builder providing one-call setup for integration tests.
//!
//! Creates an isolated test environment with mock LLM server, temp directories,
//! state manager, and tool registry, all wired together.

use std::path::PathBuf;
use tempfile::TempDir;

use crate::tools::Tool;
use shannon_engine::api::{LlmClientConfig, LlmProvider, RetryConfig};
use shannon_engine::permissions::ApprovalMode;
use shannon_engine::state::StateManager;

use crate::testing::mock_dsl::MockResponse;
#[cfg(test)]
use crate::tools::ToolOutput;

/// A fully wired test environment for Shannon integration tests.
pub struct TestShannon {
    pub base_url: String,
    pub home_dir: TempDir,
    pub workspace_dir: TempDir,
    pub recording_dir: PathBuf,
    provider: String,
    model: String,
    permission_mode: ApprovalMode,
    #[allow(dead_code)] // KEEP: test helper
    extra_tools: Vec<Box<dyn Tool>>,
    mock_responses: Vec<MockResponse>,
}

/// Builder for constructing a TestShannon environment.
pub struct TestShannonBuilder {
    provider: String,
    model: String,
    permission_mode: ApprovalMode,
    workspace_files: Vec<(String, String)>,
    extra_tools: Vec<Box<dyn Tool>>,
    with_recording: bool,
    mock_responses: Vec<MockResponse>,
}

impl Default for TestShannonBuilder {
    fn default() -> Self {
        Self {
            provider: "anthropic".to_string(),
            model: "test-model".to_string(),
            permission_mode: ApprovalMode::FullAuto,
            workspace_files: Vec::new(),
            extra_tools: Vec::new(),
            with_recording: false,
            mock_responses: Vec::new(),
        }
    }
}

impl TestShannonBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the LLM provider.
    pub fn provider(mut self, provider: &str) -> Self {
        self.provider = provider.to_string();
        self
    }

    /// Set the model name.
    pub fn model(mut self, model: &str) -> Self {
        self.model = model.to_string();
        self
    }

    /// Add a file to the workspace.
    pub fn workspace_file(mut self, path: &str, content: &str) -> Self {
        self.workspace_files
            .push((path.to_string(), content.to_string()));
        self
    }

    /// Add multiple files to the workspace.
    pub fn workspace_files(mut self, files: Vec<(&str, &str)>) -> Self {
        for (path, content) in files {
            self.workspace_files
                .push((path.to_string(), content.to_string()));
        }
        self
    }

    /// Add a custom tool to the registry.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.extra_tools.push(tool);
        self
    }

    /// Set the permission mode.
    pub fn permission_mode(mut self, mode: ApprovalMode) -> Self {
        self.permission_mode = mode;
        self
    }

    /// Enable session recording.
    pub fn with_recording(mut self) -> Self {
        self.with_recording = true;
        self
    }

    /// Pre-load mock responses for multi-turn testing.
    pub fn mock_responses(mut self, responses: Vec<MockResponse>) -> Self {
        self.mock_responses = responses;
        self
    }

    /// Build the test environment.
    pub fn build(self) -> TestShannon {
        let home_dir = TempDir::new().expect("create home temp dir");
        let workspace_dir = TempDir::new().expect("create workspace temp dir");

        // Create workspace files
        for (path, content) in &self.workspace_files {
            let full_path = workspace_dir.path().join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).expect("create parent dir");
            }
            std::fs::write(&full_path, content).expect("write workspace file");
        }

        let recording_dir = if self.with_recording {
            let dir = home_dir.path().join("recordings");
            std::fs::create_dir_all(&dir).expect("create recording dir");
            dir
        } else {
            home_dir.path().join("recordings")
        };

        TestShannon {
            base_url: String::new(),
            home_dir,
            workspace_dir,
            recording_dir,
            provider: self.provider,
            model: self.model,
            permission_mode: self.permission_mode,
            extra_tools: self.extra_tools,
            mock_responses: self.mock_responses,
        }
    }
}

impl TestShannon {
    /// Get the home directory path.
    pub fn home_path(&self) -> &std::path::Path {
        self.home_dir.path()
    }

    /// Get the workspace directory path.
    pub fn workspace_path(&self) -> &std::path::Path {
        self.workspace_dir.path()
    }

    /// Get the provider name.
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Get the model name.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the permission mode.
    pub fn permission_mode(&self) -> ApprovalMode {
        self.permission_mode
    }

    /// Read a workspace file's contents.
    pub fn read_workspace_file(&self, path: &str) -> String {
        let full_path = self.workspace_dir.path().join(path);
        std::fs::read_to_string(&full_path).unwrap_or_default()
    }

    /// Check if a workspace file exists.
    pub fn workspace_file_exists(&self, path: &str) -> bool {
        self.workspace_dir.path().join(path).exists()
    }

    /// Create the LLM client config pointing to a mock server URL.
    pub fn llm_client_config(&self, server_url: &str) -> LlmClientConfig {
        let provider = match self.provider.as_str() {
            "anthropic" => LlmProvider::Anthropic,
            "openai" => LlmProvider::OpenAI,
            "ollama" => LlmProvider::Ollama,
            "deepseek" => LlmProvider::DeepSeek,
            "groq" => LlmProvider::Groq,
            "mistral" => LlmProvider::Mistral,
            _ => LlmProvider::OpenAI,
        };

        LlmClientConfig {
            api_key: "test-key".to_string(),
            base_url: server_url.to_string(),
            model: self.model.clone(),
            max_tokens: 4096,
            timeout_seconds: 30,
            api_version: String::new(),
            provider,
            extra_headers: std::collections::HashMap::new(),
            retry_config: RetryConfig::default(),
            fallback_provider: None,
            fallback_base_url: None,
            max_stream_reconnects: 0,
            budget_tokens: None,
            reasoning_effort: None,
        }
    }

    /// Build a state manager using the home directory.
    pub fn build_state_manager(&self) -> StateManager {
        let sessions_dir = self.home_dir.path().join("sessions");
        StateManager::with_sessions_dir(sessions_dir).expect("create state manager")
    }

    /// Get the pre-loaded mock responses for multi-turn testing.
    pub fn mock_responses(&self) -> &[MockResponse] {
        &self.mock_responses
    }

    /// Take the pre-loaded mock responses (consumes them).
    pub fn take_mock_responses(&mut self) -> Vec<MockResponse> {
        std::mem::take(&mut self.mock_responses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use serde_json::{Value, json};

    /// A simple echo tool for testing.
    struct EchoTool;

    #[async_trait]
    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "Echo"
        }
        fn description(&self) -> &str {
            "Echoes input as output"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        }

        async fn execute(
            &self,
            input: Value,
        ) -> Result<ToolOutput, shannon_tool_interface::ToolError> {
            Ok(ToolOutput::success(input.to_string()))
        }
    }

    /// A tool that always fails.
    struct FailingTool {
        name: String,
        error_message: String,
    }

    #[async_trait]
    impl Tool for FailingTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "Always-failing test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            _input: Value,
        ) -> Result<ToolOutput, shannon_tool_interface::ToolError> {
            Err(shannon_tool_interface::ToolError::ExecutionFailed(
                self.error_message.clone(),
            ))
        }
    }

    /// A recordable tool that tracks calls and returns canned responses.
    struct RecordableTool {
        name: String,
        responses: std::sync::Mutex<Vec<ToolOutput>>,
        call_count: std::sync::atomic::AtomicUsize,
        call_inputs: std::sync::Mutex<Vec<Value>>,
    }

    impl RecordableTool {
        fn new(name: &str, responses: Vec<ToolOutput>) -> Self {
            Self {
                name: name.to_string(),
                responses: std::sync::Mutex::new(responses),
                call_count: std::sync::atomic::AtomicUsize::new(0),
                call_inputs: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.call_count.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn call_inputs(&self) -> Vec<Value> {
            self.call_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl Tool for RecordableTool {
        fn name(&self) -> &str {
            &self.name
        }
        fn description(&self) -> &str {
            "Recordable test tool"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(
            &self,
            input: Value,
        ) -> Result<ToolOutput, shannon_tool_interface::ToolError> {
            self.call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.call_inputs.lock().unwrap().push(input);
            let mut responses = self.responses.lock().unwrap();
            if let Some(output) = responses.pop() {
                Ok(output)
            } else {
                Ok(ToolOutput::success("default response".to_string()))
            }
        }
    }

    #[test]
    fn test_builder_defaults() {
        let builder = TestShannonBuilder::new();
        assert_eq!(builder.provider, "anthropic");
        assert_eq!(builder.model, "test-model");
    }

    #[test]
    fn test_builder_custom_settings() {
        let env = TestShannonBuilder::new()
            .provider("ollama")
            .model("llama3")
            .permission_mode(ApprovalMode::Suggest)
            .workspace_file("src/main.rs", "fn main() {}")
            .build();

        assert_eq!(env.provider(), "ollama");
        assert_eq!(env.model(), "llama3");
        assert_eq!(env.permission_mode(), ApprovalMode::Suggest);
        assert!(env.workspace_file_exists("src/main.rs"));
        assert_eq!(env.read_workspace_file("src/main.rs"), "fn main() {}");
    }

    #[test]
    fn test_workspace_files() {
        let env = TestShannonBuilder::new()
            .workspace_files(vec![
                ("src/lib.rs", "pub fn add(a: i32, b: i32) -> i32 { a + b }"),
                ("src/main.rs", "fn main() {}"),
                ("tests/test.rs", "#[test]\nfn test() {}"),
            ])
            .build();

        assert!(env.workspace_file_exists("src/lib.rs"));
        assert!(env.workspace_file_exists("src/main.rs"));
        assert!(env.workspace_file_exists("tests/test.rs"));
        assert!(!env.workspace_file_exists("src/missing.rs"));
    }

    #[test]
    fn test_llm_client_config() {
        let env = TestShannonBuilder::new().build();
        let config = env.llm_client_config("http://localhost:1234");
        assert_eq!(config.model, "test-model");
        assert_eq!(config.base_url, "http://localhost:1234");
        assert_eq!(config.api_key, "test-key");
    }

    #[test]
    fn test_build_state_manager() {
        let env = TestShannonBuilder::new().build();
        let _mgr = env.build_state_manager();
    }

    #[tokio::test]
    async fn test_recordable_tool() {
        let tool = RecordableTool::new(
            "TestTool",
            vec![
                ToolOutput::success("response 1".to_string()),
                ToolOutput::success("response 2".to_string()),
            ],
        );

        let result = tool.execute(json!({"key": "value"})).await;
        assert!(result.is_ok());
        assert_eq!(tool.call_count(), 1);
        assert_eq!(tool.call_inputs().len(), 1);
    }

    #[tokio::test]
    async fn test_failing_tool() {
        let tool = FailingTool {
            name: "FailTool".to_string(),
            error_message: "something went wrong".to_string(),
        };
        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool;
        let result = tool.execute(json!({"hello": "world"})).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.content.contains("hello"));
    }

    #[test]
    fn test_isolated_dirs() {
        let env1 = TestShannonBuilder::new().build();
        let env2 = TestShannonBuilder::new().build();

        assert_ne!(env1.home_path(), env2.home_path());
        assert_ne!(env1.workspace_path(), env2.workspace_path());
    }

    #[test]
    fn test_mock_responses() {
        use crate::testing::mock_dsl::text_response;
        let env = TestShannonBuilder::new()
            .mock_responses(vec![text_response("Hello"), text_response("World")])
            .build();

        assert_eq!(env.mock_responses().len(), 2);
    }
}
