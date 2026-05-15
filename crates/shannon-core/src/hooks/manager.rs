//! Hook manager: loads configs and executes hooks on events.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::config::{
    HookDecision, HookDef, HookResult, HookType, HooksFile,
};
use super::events::{HookEvent, HookEventType};
use super::types::HookError;

/// The hook manager that loads configs and executes hooks on events
pub struct HookManager {
    /// Combined hooks configuration (user + project level)
    hooks_file: HooksFile,
    /// Path to the user-level hooks config
    user_config_path: PathBuf,
    /// Path to the project-level hooks config
    project_config_path: PathBuf,
    /// Base directory for resolving project-level paths (defaults to cwd)
    base_dir: PathBuf,
    /// Optional LLM client for evaluating LLM-type hooks
    llm_client: Option<crate::api::client::LlmClient>,
}

impl std::fmt::Debug for HookManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookManager")
            .field("user_config_path", &self.user_config_path)
            .field("project_config_path", &self.project_config_path)
            .field("base_dir", &self.base_dir)
            .field("hooks_file", &self.hooks_file)
            .field("llm_client", &self.llm_client.is_some())
            .finish()
    }
}

impl Default for HookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl HookManager {
    /// Create a new HookManager with default paths
    pub fn new() -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| {
            eprintln!("Warning: Home directory not found, using /tmp");
            std::path::PathBuf::from("/tmp")
        });
        let base_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let user_config_path = home_dir.join(".shannon").join("hooks.json");
        let project_config_path = base_dir.join(".shannon").join("hooks.json");

        Self {
            hooks_file: HooksFile::new(),
            user_config_path,
            project_config_path,
            base_dir,
            llm_client: None,
        }
    }

    /// Create a HookManager with custom config paths (useful for testing)
    pub fn with_paths(user_path: PathBuf, project_path: PathBuf) -> Self {
        Self {
            hooks_file: HooksFile::new(),
            user_config_path: user_path,
            project_config_path: project_path,
            base_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            llm_client: None,
        }
    }

    /// Create a HookManager with an explicit base directory for project-level paths
    pub fn with_base_dir(base_dir: PathBuf) -> Self {
        let home_dir = dirs::home_dir().unwrap_or_else(|| {
            eprintln!("Warning: Home directory not found, using /tmp");
            std::path::PathBuf::from("/tmp")
        });
        let user_config_path = home_dir.join(".shannon").join("hooks.json");
        let project_config_path = base_dir.join(".shannon").join("hooks.json");

        Self {
            hooks_file: HooksFile::new(),
            user_config_path,
            project_config_path,
            base_dir,
            llm_client: None,
        }
    }

    /// Load hook configurations from disk.
    ///
    /// Loads from both Shannon-native paths and Claude Code compatible paths.
    /// Later files override earlier ones. Load order:
    ///
    /// User-level (lower priority):
    /// - `~/.claude/settings.json`
    /// - `~/.shannon/settings.json`
    /// - `~/.shannon/hooks.json`
    ///
    /// Project-level (higher priority):
    /// - `.claude/settings.json`
    /// - `.claude/settings.local.json`
    /// - `.shannon/settings.json`
    /// - `.shannon/settings.local.json`
    /// - `.shannon/hooks.json`
    pub fn load(&mut self) -> Result<(), HookError> {
        let mut combined = HooksFile::new();

        let home_dir = dirs::home_dir().ok_or(HookError::HomeNotFound)?;
        let base = &self.base_dir;

        // User-level hooks (lower priority, loaded first)
        let user_paths: Vec<PathBuf> = vec![
            home_dir.join(".claude").join("settings.json"),
            home_dir.join(".shannon").join("settings.json"),
            self.user_config_path.clone(),
        ];

        // Project-level hooks (higher priority, loaded after)
        let project_paths: Vec<PathBuf> = vec![
            base.join(".claude").join("settings.json"),
            base.join(".claude").join("settings.local.json"),
            base.join(".shannon").join("settings.json"),
            base.join(".shannon").join("settings.local.json"),
            self.project_config_path.clone(),
        ];

        for path in user_paths.iter().chain(project_paths.iter()) {
            if path.exists() {
                match HooksFile::load_from_file(path) {
                    Ok(hooks) => {
                        tracing::debug!("Loaded hooks from {}", path.display());
                        combined.merge(hooks);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load hooks from {}: {e}", path.display());
                    }
                }
            }
        }

        self.hooks_file = combined;
        Ok(())
    }

    /// Load hook configuration from a specific file path
    pub fn load_from_path(&mut self, path: &Path) -> Result<(), HookError> {
        let hooks = HooksFile::load_from_file(path)?;
        self.hooks_file = hooks;
        Ok(())
    }

    /// Run all matching hooks for a given event.
    ///
    /// Returns a vector of results from each hook that matched and was executed.
    /// If any hook returns a Deny decision, subsequent hooks for that event
    /// are still executed, but the caller should check all results for denials.
    pub async fn run_hooks(&self, event: &HookEvent) -> Result<Vec<HookResult>, HookError> {
        let event_type = event.event_type();
        let subject = event.match_subject();
        let configs = self.hooks_file.get_for_event(&event_type);

        let mut results = Vec::new();
        let event_json = event.to_json_bytes();

        for config in &configs {
            if !config.matches(&subject) {
                continue;
            }

            for hook_def in &config.hooks {
                let result = if !hook_def.blocking {
                    // For non-blocking hooks, spawn and detach
                    match &hook_def.r#type {
                        HookType::Command => {
                            self.spawn_hook(&hook_def.command, &event_json)?;
                        }
                        HookType::Http => {
                            self.spawn_http_hook(hook_def, &event_json)?;
                        }
                        HookType::Prompt => {
                            // Prompt hooks are always blocking (need LLM response)
                            let result = self.execute_prompt_hook(hook_def, &event_json).await?;
                            results.push(result);
                        }
                        HookType::McpTool | HookType::Agent => {
                            // MCP tool and agent hooks are always blocking
                            let result = self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?;
                            results.push(result);
                        }
                        HookType::Llm => {
                            // LLM hooks are always blocking (need LLM response)
                            let result = self.execute_llm_hook(hook_def, &event_json).await?;
                            results.push(result);
                        }
                    }
                    continue;
                } else {
                    match &hook_def.r#type {
                        HookType::Command => {
                            self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?
                        }
                        HookType::Http => {
                            self.execute_http_hook(hook_def, &event_json).await?
                        }
                        HookType::Prompt => {
                            self.execute_prompt_hook(hook_def, &event_json).await?
                        }
                        HookType::McpTool | HookType::Agent => {
                            self.execute_hook(&hook_def.command, hook_def.timeout_duration(), &event_json).await?
                        }
                        HookType::Llm => {
                            self.execute_llm_hook(hook_def, &event_json).await?
                        }
                    }
                };

                results.push(result);
            }
        }

        Ok(results)
    }

    /// Execute a single hook command and capture its output
    async fn execute_hook(
        &self,
        command: &str,
        timeout: Duration,
        stdin_data: &[u8],
    ) -> Result<HookResult, HookError> {
        // Validate command before execution
        if command.trim().is_empty() {
            return Err(HookError::InvalidMatcher("Hook command is empty".to_string()));
        }
        // Warn about potentially dangerous patterns in hook commands
        crate::sandbox::audit_shell_command(command);
        for pattern in &["rm -rf /", "mkfs", "dd if=", "> /dev/sd", "chmod 777"] {
            if command.contains(pattern) {
                return Err(HookError::InvalidMatcher(
                    format!("Hook command contains dangerous pattern: {pattern}")
                ));
            }
        }

        let result = tokio::time::timeout(timeout, async {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            // Write event data to stdin
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                if let Err(e) = stdin.write_all(stdin_data).await {
                    tracing::debug!("Failed to write to hook stdin: {e}");
                }
                // Drop stdin to close the pipe
                drop(stdin);
            }

            let output = child.wait_with_output().await?;

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Exit code semantics (Claude Code standard):
            //   0 = success, parse stdout JSON for decision
            //   2 = block operation, stderr shown to LLM
            //   other = non-blocking error, continue execution
            let decision = if exit_code == 2 {
                HookDecision::Deny {
                    reason: if stderr.is_empty() {
                        "Hook blocked the operation (exit 2)".to_string()
                    } else {
                        stderr.clone()
                    },
                }
            } else {
                HookResult::parse_decision(&stdout)
            };

            Ok::<HookResult, HookError>(HookResult {
                exit_code,
                stdout,
                stderr,
                decision,
                command: command.to_string(),
            })
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: command.to_string(),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Spawn a non-blocking hook (fire and forget)
    fn spawn_hook(&self, command: &str, stdin_data: &[u8]) -> Result<(), HookError> {
        let stdin_data = stdin_data.to_vec();
        let command = command.to_string();

        tokio::spawn(async move {
            match tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(mut child) => {
                    if let Some(mut stdin) = child.stdin.take() {
                        use tokio::io::AsyncWriteExt;
                        if let Err(e) = stdin.write_all(&stdin_data).await {
                            tracing::debug!("Failed to write to hook stdin: {e}");
                        }
                    }
                    if let Err(e) = child.wait().await {
                        tracing::warn!(command = %command, error = %e, "Non-blocking hook process failed");
                    }
                }
                Err(e) => {
                    tracing::warn!(command = %command, error = %e, "Failed to spawn non-blocking hook");
                }
            }
        });

        Ok(())
    }

    /// Execute an HTTP hook: POST event JSON to the configured URL
    async fn execute_http_hook(
        &self,
        hook_def: &HookDef,
        event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        let url = hook_def.url.as_deref().ok_or_else(|| {
            HookError::InvalidMatcher("HTTP hook requires a 'url' field".to_string())
        })?;

        let timeout = hook_def.timeout_duration();
        let result = tokio::time::timeout(timeout, async {
            let mut builder = reqwest::Client::new().post(url);
            for (key, value) in &hook_def.headers {
                builder = builder.header(key.as_str(), value.as_str());
            }

            let response = builder
                .header("Content-Type", "application/json")
                .body(event_json.to_vec())
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status().as_u16() as i32;
                    let body = resp.text().await.unwrap_or_default();
                    let decision = if (200..300).contains(&status) {
                        // Try parsing decision from response body
                        HookResult::parse_decision(&body)
                    } else {
                        HookDecision::Deny {
                            reason: format!("HTTP hook returned status {status}"),
                        }
                    };
                    Ok::<HookResult, HookError>(HookResult {
                        exit_code: status,
                        stdout: body,
                        stderr: String::new(),
                        decision,
                        command: format!("POST {url}"),
                    })
                }
                Err(e) => Ok::<HookResult, HookError>(HookResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: e.to_string(),
                    decision: HookDecision::Deny {
                        reason: format!("HTTP hook failed: {e}"),
                    },
                    command: format!("POST {url}"),
                }),
            }
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: format!("POST {url}"),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Spawn a non-blocking HTTP hook (fire and forget)
    fn spawn_http_hook(&self, hook_def: &HookDef, event_json: &[u8]) -> Result<(), HookError> {
        let url = hook_def.url.clone();
        let headers = hook_def.headers.clone();
        let event_json = event_json.to_vec();

        tokio::spawn(async move {
            if let Some(url) = url {
                let mut builder = reqwest::Client::new().post(&url);
                for (key, value) in &headers {
                    builder = builder.header(key.as_str(), value.as_str());
                }
                let _ = builder
                    .header("Content-Type", "application/json")
                    .body(event_json)
                    .send()
                    .await;
            }
        });

        Ok(())
    }

    /// Execute a prompt hook: single-turn LLM evaluation
    ///
    /// The command is treated as a prompt template. The event JSON is appended
    /// as context. The LLM's first line of output is parsed as a HookDecision.
    async fn execute_prompt_hook(
        &self,
        hook_def: &HookDef,
        event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        // For prompt hooks, we invoke the command as a shell command but
        // pass the event JSON and expect a JSON decision back.
        // This allows using any CLI tool that can evaluate prompts.
        let timeout = hook_def.timeout_duration();
        let command = &hook_def.command;

        let result = tokio::time::timeout(timeout, async {
            let mut child = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(command)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?;

            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                if let Err(e) = stdin.write_all(event_json).await {
                    tracing::debug!("Failed to write to hook stdin: {e}");
                }
                drop(stdin);
            }

            let output = child.wait_with_output().await?;
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Prompt hooks: exit code 2 = deny
            let decision = if exit_code == 2 {
                HookDecision::Deny {
                    reason: if stderr.is_empty() {
                        "Prompt hook denied".to_string()
                    } else {
                        stderr.clone()
                    },
                }
            } else {
                HookResult::parse_decision(&stdout)
            };

            Ok::<HookResult, HookError>(HookResult {
                exit_code,
                stdout,
                stderr,
                decision,
                command: format!("prompt: {command}"),
            })
        })
        .await;

        match result {
            Ok(Ok(hook_result)) => Ok(hook_result),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(HookError::Timeout {
                command: hook_def.command.clone(),
                timeout_secs: timeout.as_secs(),
            }),
        }
    }

    /// Execute an LLM hook: LLM-powered hook evaluation
    ///
    /// LLM hooks use an LLM to evaluate hook events and make decisions.
    /// Sends the hook's prompt template (with variable substitution) along with
    /// the event JSON to the LLM, then parses the response as a HookDecision.
    async fn execute_llm_hook(
        &self,
        hook_def: &HookDef,
        event_json: &[u8],
    ) -> Result<HookResult, HookError> {
        let timeout = hook_def.timeout_duration();
        let command_label = format!("llm: {}", hook_def.prompt_template.as_deref().unwrap_or(&hook_def.command));

        let client = match &self.llm_client {
            Some(c) => c.clone(),
            None => {
                tracing::warn!("LLM hook executed but no LlmClient configured, denying by default");
                return Ok(HookResult {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: "No LLM client configured for hook evaluation".to_string(),
                    decision: HookDecision::Deny {
                        reason: "LLM hook requires LlmClient but none is configured".to_string(),
                    },
                    command: command_label,
                });
            }
        };

        // Build the user prompt from the template or fall back to the command field
        let event_json_str = String::from_utf8_lossy(event_json);
        let user_prompt = match &hook_def.prompt_template {
            Some(template) => template
                .replace("{{event_json}}", &event_json_str)
                .replace("{{event_type}}", &Self::extract_event_type_from_json(&event_json_str)),
            None => hook_def.command.clone(),
        };

        let system_prompt = "\
You are a hook evaluator for a code assistant. You receive an event and must decide \
whether to allow or deny the operation.

Respond with EXACTLY one of these JSON objects on a SINGLE line, nothing else:
- {\"decision\":\"allow\"}
- {\"decision\":\"deny\",\"reason\":\"<brief reason>\"}

Do not include any other text, explanation, or formatting.";

        let messages = vec![crate::api::types::Message {
            role: "user".to_string(),
            content: crate::api::types::MessageContent::Text(user_prompt),
        }];

        // If the hook specifies a model override, clone the client and swap the model
        let client = match &hook_def.model {
            Some(model) => {
                let mut c = client;
                c.set_model(model.clone());
                c
            }
            None => client,
        };

        let result = tokio::time::timeout(timeout, async {
            client
                .send_message(messages, None, Some(system_prompt.to_string()))
                .await
        })
        .await;

        match result {
            Ok(Ok(content_blocks)) => {
                // Extract text from response content blocks
                let response_text = content_blocks
                    .iter()
                    .filter_map(|block| match block {
                        crate::api::types::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");

                let decision = HookResult::parse_decision(&response_text);
                Ok(HookResult {
                    exit_code: 0,
                    stdout: response_text,
                    stderr: String::new(),
                    decision,
                    command: command_label,
                })
            }
            Ok(Err(api_err)) => {
                tracing::warn!("LLM hook call failed: {api_err}, denying by default");
                Ok(HookResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("LLM call failed: {api_err}"),
                    decision: HookDecision::Deny {
                        reason: format!("LLM hook evaluation failed: {api_err}"),
                    },
                    command: command_label,
                })
            }
            Err(_) => {
                tracing::warn!("LLM hook timed out after {}s, denying by default", timeout.as_secs());
                Ok(HookResult {
                    exit_code: -1,
                    stdout: String::new(),
                    stderr: format!("LLM hook timed out after {}s", timeout.as_secs()),
                    decision: HookDecision::Deny {
                        reason: format!("LLM hook timed out after {}s", timeout.as_secs()),
                    },
                    command: command_label,
                })
            }
        }
    }

    /// Extract the event type value from a JSON payload for template substitution.
    fn extract_event_type_from_json(json_str: &str) -> String {
        serde_json::from_str::<Value>(json_str)
            .ok()
            .and_then(|v| {
                // HookEvent is serialized with PascalCase; iterate keys looking
                // for a known event-type object and return its key name.
                v.as_object()
                    .and_then(|obj| obj.keys().next().cloned())
            })
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Process hook results and determine the final outcome.
    ///
    /// Returns the combined decision:
    /// - If any hook denies, returns the first denial reason.
    /// - If any hook modifies, returns the last modification.
    /// - Otherwise, returns Allow.
    pub fn resolve_results(results: &[HookResult]) -> HookDecision {
        let mut last_modify = None;

        for result in results {
            match &result.decision {
                HookDecision::Deny { reason } => {
                    return HookDecision::Deny {
                        reason: reason.clone(),
                    };
                }
                HookDecision::Modify {
                    modified_input,
                    modified_output,
                } => {
                    last_modify = Some(HookDecision::Modify {
                        modified_input: modified_input.clone(),
                        modified_output: modified_output.clone(),
                    });
                }
                HookDecision::Allow => {}
            }
        }

        last_modify.unwrap_or(HookDecision::Allow)
    }

    /// Get the user config path
    pub fn user_config_path(&self) -> &Path {
        &self.user_config_path
    }

    /// Get the project config path
    pub fn project_config_path(&self) -> &Path {
        &self.project_config_path
    }

    /// Get a reference to the loaded hooks file
    pub fn hooks_file(&self) -> &HooksFile {
        &self.hooks_file
    }

    /// Get the list of event types that have configured hooks
    pub fn configured_event_types(&self) -> Vec<HookEventType> {
        self.hooks_file
            .hooks
            .keys()
            .filter_map(|k| HookEventType::from_str_lossy(k))
            .collect()
    }

    /// Set the LLM client used for evaluating LLM-type hooks
    pub fn set_llm_client(&mut self, client: crate::api::client::LlmClient) {
        self.llm_client = Some(client);
    }
}
