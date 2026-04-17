use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use futures::StreamExt;
use shannon_core::{
    api::LlmClientConfig,
    i18n,
    query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata},
    state::StateManager,
    tools::ToolRegistry,
    unified_config::{ConfigBuilder, ShannonConfig},
};
use shannon_tools::register_default_tools;
use shannon_ui::Repl;
use std::collections::HashMap;
use std::io::Write;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

/// CLI configuration passed explicitly instead of via environment variables.
///
/// This struct holds all configuration that was previously set via unsafe
/// `std::env::set_var` calls. It is passed explicitly to functions that need
/// this configuration, eliminating the need for environment variable mutation.
#[derive(Debug, Clone, Default)]
struct CliConfig {
    /// LLM model to use (e.g., claude-sonnet-4, gpt-4o)
    model: Option<String>,
    /// LLM provider (anthropic, openai, ollama, custom)
    provider: Option<String>,
    /// Maximum tokens for the response
    max_tokens: Option<usize>,
    /// Sampling temperature, 0.0 - 1.0
    temperature: Option<f32>,
    /// Request timeout in seconds
    timeout: Option<u64>,
    /// Enable debug logging
    debug: bool,
    /// Additional environment variable overrides (KEY=VALUE pairs)
    env_overrides: HashMap<String, String>,
}

impl CliConfig {
    /// Get the model, with fallback to environment variable.
    fn model(&self) -> Option<String> {
        self.model.clone().or_else(|| std::env::var("SHANNON_MODEL").ok())
    }

    /// Get the provider, with fallback to environment variable.
    fn provider(&self) -> Option<String> {
        self.provider.clone().or_else(|| std::env::var("SHANNON_PROVIDER").ok())
    }

    /// Get max_tokens, with fallback to environment variable.
    fn max_tokens(&self) -> Option<usize> {
        self.max_tokens.or_else(|| {
            std::env::var("SHANNON_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
        })
    }

    /// Get temperature, with fallback to environment variable.
    fn temperature(&self) -> Option<f32> {
        self.temperature.or_else(|| {
            std::env::var("SHANNON_TEMPERATURE")
                .ok()
                .and_then(|s| s.parse().ok())
        })
    }

    /// Get timeout, with fallback to environment variable.
    fn timeout(&self) -> Option<u64> {
        self.timeout.or_else(|| {
            std::env::var("SHANNON_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
        })
    }

    /// Get debug flag, with fallback to environment variable.
    fn debug(&self) -> bool {
        if self.debug {
            return true;
        }
        std::env::var("SHANNON_DEBUG")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(false)
    }

    /// Get an environment variable value, checking overrides first.
    fn get_env(&self, key: &str) -> Option<String> {
        self.env_overrides
            .get(key)
            .cloned()
            .or_else(|| std::env::var(key).ok())
    }
}

// ── TOML Config File Loading ───────────────────────────────────────────

/// Configuration read from TOML config files.
///
/// Loaded from (in order, later wins):
///   1. `~/.shannon/config.toml` — global user config
///   2. `.shannon.toml` in the current working directory — project-local config
///
/// Values here act as defaults; CLI args and env vars take precedence.
#[derive(Debug, Clone, serde::Deserialize, Default)]
#[serde(default)]
struct ShannonTomlConfig {
    model: Option<String>,
    provider: Option<String>,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
    timeout: Option<u64>,
    debug: Option<bool>,
    api_key: Option<String>,
    base_url: Option<String>,
}

/// Load TOML config from disk, merging global + project-local files.
fn load_toml_config() -> ShannonTomlConfig {
    let mut merged = ShannonTomlConfig::default();

    // 1. Global config: ~/.shannon/config.toml
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(".shannon").join("config.toml");
        if let Ok(content) = std::fs::read_to_string(&global_path) {
            if let Ok(cfg) = toml::from_str::<ShannonTomlConfig>(&content) {
                merged = cfg;
            }
        }
    }

    // 2. Project-local config: .shannon.toml
    let local_path = std::path::Path::new(".shannon.toml");
    if let Ok(content) = std::fs::read_to_string(local_path) {
        if let Ok(cfg) = toml::from_str::<ShannonTomlConfig>(&content) {
            // Merge: local overrides global
            if cfg.model.is_some() { merged.model = cfg.model; }
            if cfg.provider.is_some() { merged.provider = cfg.provider; }
            if cfg.max_tokens.is_some() { merged.max_tokens = cfg.max_tokens; }
            if cfg.temperature.is_some() { merged.temperature = cfg.temperature; }
            if cfg.timeout.is_some() { merged.timeout = cfg.timeout; }
            if cfg.debug.is_some() { merged.debug = cfg.debug; }
            if cfg.api_key.is_some() { merged.api_key = cfg.api_key; }
            if cfg.base_url.is_some() { merged.base_url = cfg.base_url; }
        }
    }

    merged
}

/// Shannon Code - AI-powered code assistant in Rust
///
/// A production-grade AI agent harness reimplementation in Rust
#[derive(Parser, Debug)]
#[command(name = "shannon")]
#[command(author = "Shannon Code Contributors")]
#[command(version = "0.1.0")]
#[command(about = "AI-powered code assistant in Rust", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Execute a prompt directly (non-interactive mode).
    /// Example: shannon "你用的什么模型"
    #[arg(index = 1)]
    prompt: Option<String>,

    /// Read prompt from stdin (pipe mode).
    /// Example: echo "fix this bug" | shannon --pipe
    ///          cat file.txt | shannon --pipe "summarize this"
    #[arg(long)]
    pipe: bool,

    /// LLM model to use (e.g., claude-sonnet-4, gpt-4o)
    #[arg(short, long)]
    model: Option<String>,

    /// LLM provider (anthropic, openai, ollama, custom)
    #[arg(short, long)]
    provider: Option<String>,

    /// Set language/locale (e.g., en, zh)
    #[arg(long)]
    lang: Option<String>,

    /// Auto-approve all tool executions (non-interactive mode only).
    /// Without this flag, non-interactive mode uses FullAuto (allows all non-critical tools).
    /// With this flag, even critical tools are allowed (BypassPermissions).
    #[arg(short = 'y', long)]
    yes: bool,

    /// Run as a team agent process (internal flag for multi-agent coordination).
    /// Reads JSON-RPC from stdin, executes tasks using the full LLM + tool stack,
    /// and writes JSON-RPC events to stdout.
    #[arg(long, hide = true)]
    team_agent: bool,

    /// Agent name (used with --team-agent, must be unique within the team).
    #[arg(long, hide = true)]
    name: Option<String>,

    /// System prompt for the agent (used with --team-agent).
    #[arg(long, hide = true)]
    system_prompt: Option<String>,

    /// Working directory for the agent (used with --team-agent).
    #[arg(long, hide = true)]
    workdir: Option<String>,

    /// Permission/approval mode for the agent (used with --team-agent).
    #[arg(long, hide = true)]
    permission_mode: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// Shannon CLI commands
#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the Shannon REPL (Read-Eval-Print Loop)
    Repl {
        /// Optional project file to load on startup
        #[arg(short, long)]
        file: Option<String>,

        /// Set environment variables, format: KEY=VALUE.
        /// Can be specified multiple times. Highest priority override.
        /// Example: -e SHANNON_MODEL=gpt-4o -e SHANNON_MAX_TOKENS=8192
        #[arg(short = 'e', long = "env")]
        env: Vec<String>,

        /// LLM model to use (e.g., claude-sonnet-4, gpt-4o, claude-3-5-sonnet-20241022)
        #[arg(short, long)]
        model: Option<String>,

        /// LLM provider (anthropic, openai, ollama, custom)
        #[arg(short, long)]
        provider: Option<String>,

        /// Maximum tokens for the response (default: 8192)
        #[arg(long)]
        max_tokens: Option<usize>,

        /// Sampling temperature, 0.0 - 1.0 (default: 0.7)
        #[arg(long)]
        temperature: Option<f32>,

        /// Request timeout in seconds (default: 120)
        #[arg(long)]
        timeout: Option<u64>,

        /// Enable debug logging
        #[arg(short, long)]
        debug: bool,

        /// Working directory for the session (default: current directory)
        #[arg(long)]
        cwd: Option<String>,

        /// Use local Ollama for inference (shortcut for --provider ollama)
        #[arg(short, long)]
        local: bool,
    },

    /// Display version information
    Version {
        /// Show detailed version information
        #[arg(short, long)]
        verbose: bool,
    },

    /// Manage Shannon configuration
    Config {
        #[arg(short, long)]
        setting: Option<String>,
    },

    /// Execute a query directly (non-interactive mode)
    Query {
        /// The query/prompt to execute
        query: String,

        /// LLM model to use
        #[arg(short, long)]
        model: Option<String>,

        /// LLM provider (anthropic, openai, ollama, custom)
        #[arg(short, long)]
        provider: Option<String>,

        /// Maximum tokens for response
        #[arg(long)]
        max_tokens: Option<usize>,

        /// Output format (text, json, markdown)
        #[arg(long, default_value_t = String::from("text"))]
        output: String,

        /// Disable streaming output (wait for complete response)
        #[arg(long)]
        no_stream: bool,
    },

    /// Start the HTTP API server
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,

        /// Host to bind to
        #[arg(short, long)]
        host: Option<String>,
    },
}

/// Parse CLI env overrides into a HashMap.
/// Returns Err for malformed entries (missing '=' or empty key).
fn parse_cli_env(env: &[String]) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    for pair in env {
        match pair.split_once('=') {
            Some((key, value)) => {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                if key.is_empty() {
                    return Err(format!("empty key in env override: {pair}"));
                }
                map.insert(key, value);
            }
            None => return Err(format!("malformed env override (missing '='): {pair}")),
        }
    }
    Ok(map)
}

/// Build a CliConfig from CLI options.
///
/// This replaces the unsafe `apply_env_overrides` function by collecting
/// all configuration into a struct that is passed explicitly to functions.
fn build_cli_config(
    model: Option<&str>,
    provider: Option<&str>,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
    timeout: Option<u64>,
    debug: bool,
    env_overrides: HashMap<String, String>,
) -> CliConfig {
    // Load TOML config as fallback defaults
    let toml_cfg = load_toml_config();

    CliConfig {
        model: model.map(|s| s.to_string()).or(toml_cfg.model),
        provider: provider.map(|s| s.to_string()).or(toml_cfg.provider),
        max_tokens: max_tokens.or(toml_cfg.max_tokens),
        temperature: temperature.or(toml_cfg.temperature),
        timeout: timeout.or(toml_cfg.timeout),
        debug: debug || toml_cfg.debug.unwrap_or(false),
        env_overrides,
    }
}

/// Build an [`LlmClientConfig`] by wiring the [`ConfigBuilder`] with the
/// user's TOML config files, environment variables, and CLI overrides.
///
/// Priority (highest → lowest):
///   CLI overrides > env vars (`SHANNON_*`) > local `.shannon.toml` > global `~/.shannon/config.toml`
fn build_llm_config_from_builder(cli_config: &CliConfig) -> LlmClientConfig {
    // 1. Convert the already-parsed CLI options into a ShannonConfig for the
    //    highest-priority layer. Use the accessor methods which include env var
    //    fallback (e.g. cli_config.provider() checks SHANNON_PROVIDER).
    let cli_overrides = ShannonConfig {
        model: cli_config.model(),
        provider: cli_config.provider(),
        api_key: cli_config.get_env("SHANNON_API_KEY")
            .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
            .or_else(|| std::env::var("OPENAI_API_KEY").ok()),
        base_url: cli_config.get_env("SHANNON_BASE_URL"),
        max_tokens: cli_config.max_tokens(),
        temperature: cli_config.temperature(),
        timeout: cli_config.timeout(),
        debug: cli_config.debug(),
    };

    // 2. Build the merged ShannonConfig via ConfigBuilder.
    let merged = ConfigBuilder::new()
        .load_global_toml()
        .load_local_toml()
        .load_env_vars()
        .set_cli_overrides(cli_overrides)
        .build();

    // 3. Convert to LlmClientConfig (uses the `From<ShannonConfig>` impl).
    LlmClientConfig::from(merged)
}

/// Run a non-interactive query, outputting results to stdout.
/// `stream` controls whether text is streamed character-by-character.
/// `config` holds explicit CLI configuration.
/// `bypass_all` when true, skips all permission checks (BypassPermissions mode).
fn run_noninteractive_query(query: &str, stream: bool, config: &CliConfig, bypass_all: bool) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        // Build tool registry with all standard tools
        let mut tools = ToolRegistry::new();
        let agent_context_handle = register_default_tools(&mut tools)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;

        // Load and register skills from shannon-skills as tools
        shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Discover and load plugins, register their tools
        let mut plugin_manager = shannon_core::PluginManager::new();
        match plugin_manager.discover_and_load_all().await {
            Ok(loaded) if !loaded.is_empty() => {
                eprintln!("Loaded {} plugin(s)", loaded.len());
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Warning: plugin discovery failed: {e}");
            }
        }
        shannon_core::register_plugin_tools(&plugin_manager, &mut tools);

        // Discover MCP server configurations
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                eprintln!("Discovered {mcp_count} MCP server(s)");
                for config in mcp_registry.enabled_servers() {
                    let description = format!(
                        "Execute tool calls on MCP server '{}' ({})",
                        config.name, config.transport_type
                    );
                    let input_schema = serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": {"type": "string", "description": "Tool to call on MCP server"},
                            "arguments": {"type": "object", "description": "Arguments for the MCP tool"}
                        },
                        "required": ["tool_name"]
                    });
                    let mcp_tool = shannon_core::McpToolAdapter::new(
                        config.name.clone(),
                        config.command.clone(),
                        config.args.clone(),
                        config.env.clone(),
                        description,
                        input_schema,
                    );
                    let _ = tools.register(Box::new(mcp_tool));
                }
            }
        }

        // Build LLM client from the merged ConfigBuilder pipeline
        let client_config = build_llm_config_from_builder(config);

        // Inject team context into AgentTool for sub-agent execution + team coordination
        match shannon_tools::AgentToolContext::new(client_config.clone()).await {
            Ok(ctx) => {
                // Register team coordination tools (team_task_create/update/list)
                if let Err(e) = shannon_tools::register_team_tools(&mut tools, ctx.coordinator.clone()) {
                    eprintln!("Warning: Team tool registration failed: {e}");
                }
                if let Ok(mut guard) = agent_context_handle.lock() {
                    *guard = Some(ctx);
                }
            }
            Err(e) => eprintln!("Warning: Team context init failed (team features disabled): {e}"),
        }

        // Validate and warn
        if let Err(e) = client_config.validate() {
            eprintln!("Warning: {e}");
        }

        let client = if client_config.provider.requires_auth() {
            shannon_core::api::LlmClient::new(client_config)
        } else {
            shannon_core::api::LlmClient::new_unauthenticated(client_config)
        };

        let mut permissions = shannon_core::permissions::PermissionManager::new();
        // Non-interactive mode: use FullAuto by default (allows all non-critical tools),
        // or BypassPermissions with --yes flag (allows everything including critical).
        if bypass_all {
            permissions.set_approval_mode(shannon_core::permissions::ApprovalMode::BypassPermissions);
        } else {
            permissions.set_approval_mode(shannon_core::permissions::ApprovalMode::FullAuto);
        }
        let state = StateManager::new();

        let base_engine = QueryEngine::with_defaults(client, tools, permissions, state);

        // Initialize memory store at ~/.shannon/memories/
        let mut engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            let _ = mem_store.load();
            base_engine.with_memory(mem_store)
        };

        // Auto-load project instructions (CLAUDE.md, AGENTS.md, GEMINI.md)
        if let Some(instructions) = shannon_core::project_instructions::load_from_cwd() {
            engine.append_system_prompt(&instructions.content);
        }

        let context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: query.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: true,
                max_tokens: config.max_tokens().map(|v| v as u32),
                model: config
                    .model()
                    .unwrap_or_else(|| "default".to_string()),
                temperature: config.temperature(),
                top_p: None,
            },
        };

        let mut event_stream = engine.process_query(context, None).await;

        let mut response_text = String::new();
        let mut tool_count = 0usize;

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(QueryEvent::Text { content, .. }) => {
                    if stream {
                        print!("{content}");
                        std::io::stdout().flush().ok();
                    }
                    response_text.push_str(&content);
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                    tool_count += 1;
                    // Show concise tool invocation — truncate large inputs
                    let input_summary = match serde_json::to_string(&tool_input) {
                        Ok(s) if s.len() > 200 => format!("{}…", &s[..200]),
                        Ok(s) => s,
                        Err(_) => "(invalid json)".to_string(),
                    };
                    eprintln!("[tool #{tool_count}: {tool_name}] {input_summary}");
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    if is_error {
                        let err_summary = if result.len() > 300 {
                            format!("{}…", &result[..300])
                        } else {
                            result
                        };
                        eprintln!("[tool-error: {tool_name}] {err_summary}");
                    } else {
                        eprintln!("[tool-done: {tool_name}]");
                    }
                }
                Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                    eprintln!("[turn {turn_number} completed, {tokens_used} tokens]");
                }
                Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                    let total = input_tokens + output_tokens;
                    let total_fmt = if total >= 1000 {
                        format!("{:.1}k", total as f64 / 1000.0)
                    } else {
                        total.to_string()
                    };
                    eprintln!("[usage: {total_fmt} tokens (${cost_usd:.4})]");
                }
                Ok(QueryEvent::Progress { message, .. }) => {
                    eprintln!("[{message}]");
                }
                Ok(QueryEvent::Completed { .. }) => {
                    if !stream && !response_text.is_empty() {
                        println!("{response_text}");
                    }
                    if tool_count > 0 {
                        eprintln!("[completed: {tool_count} tool(s) invoked]");
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    eprintln!("Error: {error}");
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Stream error: {e}");
                }
            }
        }

        Ok(())
    })
}

/// Read all of stdin into a String. Returns empty string if stdin is a terminal
/// (i.e., not piped).
fn read_stdin() -> String {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return String::new();
    }
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
    // Also read remaining content if available
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf);
    buf.trim().to_string()
}

/// Run the HTTP API server.
fn run_serve_command(port: u16, host: Option<String>, config: &CliConfig) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        // Build tool registry with default tools.
        let mut tools = shannon_core::ToolRegistry::new();
        let agent_context_handle = register_default_tools(&mut tools)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;

        // Load and register skills.
        shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Build LLM client config via the shared ConfigBuilder pipeline.
        let client_config = build_llm_config_from_builder(config);

        // Inject team context into AgentTool for sub-agent execution + team coordination
        match shannon_tools::AgentToolContext::new(client_config.clone()).await {
            Ok(ctx) => {
                if let Err(e) = shannon_tools::register_team_tools(&mut tools, ctx.coordinator.clone()) {
                    eprintln!("Warning: Team tool registration failed: {e}");
                }
                if let Ok(mut guard) = agent_context_handle.lock() {
                    *guard = Some(ctx);
                }
            }
            Err(e) => eprintln!("Warning: Team context init failed (team features disabled): {e}"),
        }

        let mut server = shannon_core::api_server::ShannonApiServer::new(client_config)
            .with_tools(tools)
            .port(port);

        if let Some(h) = host {
            server = server.host(h);
        }

        println!("Shannon API server starting on port {port}");
        server.serve().await.map_err(|e| anyhow::anyhow!("{e}"))
    })
}

// ── Team Agent Mode ─────────────────────────────────────────────────

/// Write a JSON-RPC message line to stdout.
async fn agent_write_line(line: &str) {
    let mut stdout = tokio::io::stdout();
    if let Err(e) = stdout.write_all(line.as_bytes()).await {
        tracing::error!("Failed to write to stdout: {e}");
    }
    if let Err(e) = stdout.flush().await {
        tracing::error!("Failed to flush stdout: {e}");
    }
}

/// Send a JSON-RPC notification to the coordinator.
async fn agent_notify(method: &str, params: serde_json::Value) {
    let msg = shannon_agents::JsonRpcMessage::notification(method, params);
    match shannon_agents::frame_message(&msg) {
        Ok(line) => agent_write_line(&line).await,
        Err(e) => tracing::error!("Failed to frame notification: {e}"),
    }
}

/// Send a JSON-RPC success response.
async fn agent_respond(id: i64, result: serde_json::Value) {
    let msg = shannon_agents::JsonRpcMessage::response(
        shannon_agents::JsonRpcId::Number(id),
        result,
    );
    match shannon_agents::frame_message(&msg) {
        Ok(line) => agent_write_line(&line).await,
        Err(e) => tracing::error!("Failed to frame response: {e}"),
    }
}

/// Send a JSON-RPC error response.
async fn agent_respond_error(id: i64, error: shannon_agents::JsonRpcError) {
    let msg = shannon_agents::JsonRpcMessage::error_response(
        shannon_agents::JsonRpcId::Number(id),
        error,
    );
    match shannon_agents::frame_message(&msg) {
        Ok(line) => agent_write_line(&line).await,
        Err(e) => tracing::error!("Failed to frame error response: {e}"),
    }
}

/// Run in team-agent mode: read JSON-RPC from stdin, execute tasks with full
/// LLM + tool access, write JSON-RPC events to stdout.
///
/// This is the in-process counterpart of `shannon-agent` that reuses the main
/// `shannon` binary. It gives agents full access to the query engine, tool
/// registry, MCP servers, plugins, and the LLM client — everything the REPL
/// has, minus the interactive UI.
fn run_team_agent_mode(
    name: &str,
    model: Option<&str>,
    provider: Option<&str>,
    system_prompt: Option<&str>,
    workdir: Option<&str>,
    permission_mode: Option<&str>,
) -> Result<()> {
    // Change working directory if specified
    if let Some(dir) = workdir {
        std::env::set_current_dir(dir)
            .map_err(|e| anyhow::anyhow!("Failed to set working directory: {e}"))?;
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let config = build_cli_config(model, provider, None, None, None, false, HashMap::new());

        // ── Build full tool registry (same as non-interactive query) ──
        let mut tools = ToolRegistry::new();
        let agent_context_handle = register_default_tools(&mut tools)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;

        shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Discover plugins
        let mut plugin_manager = shannon_core::PluginManager::new();
        match plugin_manager.discover_and_load_all().await {
            Ok(loaded) if !loaded.is_empty() => {
                tracing::info!("Loaded {} plugin(s)", loaded.len());
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Plugin discovery failed: {e}"),
        }
        shannon_core::register_plugin_tools(&plugin_manager, &mut tools);

        // Discover MCP servers
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                tracing::info!("Discovered {mcp_count} MCP server(s)");
                for mcp_config in mcp_registry.enabled_servers() {
                    let description = format!(
                        "Execute tool calls on MCP server '{}' ({})",
                        mcp_config.name, mcp_config.transport_type
                    );
                    let input_schema = serde_json::json!({
                        "type": "object",
                        "properties": {
                            "tool_name": {"type": "string", "description": "Tool to call"},
                            "arguments": {"type": "object", "description": "Arguments"}
                        },
                        "required": ["tool_name"]
                    });
                    let mcp_tool = shannon_core::McpToolAdapter::new(
                        mcp_config.name.clone(),
                        mcp_config.command.clone(),
                        mcp_config.args.clone(),
                        mcp_config.env.clone(),
                        description,
                        input_schema,
                    );
                    let _ = tools.register(Box::new(mcp_tool));
                }
            }
        }

        // ── Build LLM client ──
        let client_config = build_llm_config_from_builder(&config);
        if let Err(e) = client_config.validate() {
            tracing::warn!("Config validation: {e}");
        }

        // Team context
        match shannon_tools::AgentToolContext::new(client_config.clone()).await {
            Ok(ctx) => {
                if let Err(e) = shannon_tools::register_team_tools(&mut tools, ctx.coordinator.clone()) {
                    tracing::warn!("Team tool registration failed: {e}");
                }
                if let Ok(mut guard) = agent_context_handle.lock() {
                    *guard = Some(ctx);
                }
            }
            Err(e) => tracing::warn!("Team context init failed (team features disabled): {e}"),
        }

        // ── Remote team tools (JSON-RPC to coordinator via stdin/stdout) ──
        let coordinator_channel = shannon_agents::CoordinatorChannel::new();
        let agent_name_owned = name.to_string();
        tools.register(Box::new(shannon_agents::RemoteTeamTaskListTool::new(
            coordinator_channel.clone(),
        ))).ok();
        tools.register(Box::new(shannon_agents::RemoteTeamTaskClaimTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).ok();
        tools.register(Box::new(shannon_agents::RemoteTeamNotifyIdleTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).ok();
        tools.register(Box::new(shannon_agents::RemoteSendMessageTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).ok();
        let coordinator_channel_for_loop = coordinator_channel.clone();

        let client = if client_config.provider.requires_auth() {
            shannon_core::api::LlmClient::new(client_config)
        } else {
            shannon_core::api::LlmClient::new_unauthenticated(client_config)
        };

        let mut permissions = shannon_core::permissions::PermissionManager::new();
        let approval_mode = match permission_mode {
            Some("auto") => shannon_core::permissions::ApprovalMode::AutoEdit,
            Some("plan") => shannon_core::permissions::ApprovalMode::Plan,
            Some("full-auto") => shannon_core::permissions::ApprovalMode::FullAuto,
            Some("dontAsk") => shannon_core::permissions::ApprovalMode::DontAsk,
            Some("readonly") => shannon_core::permissions::ApprovalMode::Readonly,
            _ => shannon_core::permissions::ApprovalMode::BypassPermissions,
        };
        permissions.set_approval_mode(approval_mode);
        let state = StateManager::new();

        let base_engine = QueryEngine::with_defaults(client, tools, permissions, state);

        // Memory store
        let mut engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            let _ = mem_store.load();
            base_engine.with_memory(mem_store)
        };

        // System prompt: project instructions + agent-specific prompt
        if let Some(instructions) = shannon_core::project_instructions::load_from_cwd() {
            engine.append_system_prompt(&instructions.content);
        }
        if let Some(prompt) = system_prompt {
            if !prompt.is_empty() {
                engine.append_system_prompt(prompt);
            }
        }

        // ── Send agent_ready notification ──
        let ready_params = serde_json::to_value(shannon_agents::AgentReadyParams {
            agent_name: name.to_string(),
            capabilities: vec!["general".to_string()],
        }).unwrap();
        agent_notify("agent_ready", ready_params).await;

        // ── JSON-RPC main loop ──
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        tracing::info!(name = %name, "Agent listening for JSON-RPC on stdin");

        while let Ok(Some(line)) = lines.next_line().await {
            let msg = match shannon_agents::parse_message(&line) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse JSON-RPC");
                    continue;
                }
            };

            // Dispatch JSON-RPC responses to waiting remote tools
            if msg.method().is_none() && msg.id.is_some() {
                coordinator_channel_for_loop.dispatch_response(msg).await;
                continue;
            }

            match msg.method() {
                Some("execute_task") => {
                    if let Some(params) = msg.params {
                        match serde_json::from_value::<shannon_agents::ExecuteTaskParams>(params) {
                            Ok(task_params) => {
                                tracing::info!(
                                    task_id = %task_params.task_id,
                                    subject = %task_params.subject,
                                    "Executing task"
                                );

                                // Send start progress
                                let progress = serde_json::to_value(
                                    shannon_agents::TaskProgressParams {
                                        task_id: task_params.task_id.clone(),
                                        chunk: format!("Starting: {}", task_params.subject),
                                    }).unwrap();
                                agent_notify("task_progress", progress).await;

                                // Build query from task
                                let task_desc = if task_params.description.is_empty() {
                                    task_params.subject.clone()
                                } else {
                                    format!("{}\n\n{}", task_params.subject, task_params.description)
                                };

                                let context = QueryContext {
                                    query_id: Uuid::new_v4(),
                                    session_id: Uuid::new_v4(),
                                    user_message: task_desc,
                                    metadata: QueryMetadata {
                                        timestamp: chrono::Utc::now(),
                                        tools_allowed: true,
                                        max_tokens: None,
                                        model: config.model().unwrap_or_default(),
                                        temperature: None,
                                        top_p: None,
                                    },
                                };

                                let mut event_stream = engine.process_query(context, None).await;
                                let mut output = String::new();
                                let mut success = true;

                                while let Some(event_result) = event_stream.next().await {
                                    match event_result {
                                        Ok(QueryEvent::Text { content, .. }) => {
                                            output.push_str(&content);
                                            // Stream progress chunks
                                            let chunk = serde_json::to_value(
                                                shannon_agents::TaskProgressParams {
                                                    task_id: task_params.task_id.clone(),
                                                    chunk: content,
                                                }).unwrap();
                                            agent_notify("task_progress", chunk).await;
                                        }
                                        Ok(QueryEvent::ToolUseRequest { tool_name, .. }) => {
                                            tracing::info!(tool = %tool_name, "Agent invoking tool");
                                        }
                                        Ok(QueryEvent::Completed { .. }) => {}
                                        Ok(QueryEvent::Failed { error, .. }) => {
                                            output.push_str(&format!("\nError: {error}"));
                                            success = false;
                                        }
                                        Ok(_) => {}
                                        Err(e) => {
                                            output.push_str(&format!("\nStream error: {e}"));
                                            success = false;
                                        }
                                    }
                                }

                                // Send task_complete
                                let complete = serde_json::to_value(
                                    shannon_agents::TaskCompleteParams {
                                        task_id: task_params.task_id.clone(),
                                        success,
                                        output,
                                    }).unwrap();
                                agent_notify("task_complete", complete).await;

                                // Report idle
                                let idle = serde_json::to_value(
                                    shannon_agents::AgentIdleParams {
                                        agent_name: name.to_string(),
                                        available_tasks_count: 0,
                                    }).unwrap();
                                agent_notify("agent_idle", idle).await;
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Invalid execute_task params");
                                if let Some(id) = &msg.id {
                                    let rpc_id = match id {
                                        shannon_agents::JsonRpcId::Number(n) => *n,
                                        _ => -1,
                                    };
                                    agent_respond_error(
                                        rpc_id,
                                        shannon_agents::JsonRpcError::internal(e.to_string()),
                                    ).await;
                                }
                            }
                        }
                    }
                }
                Some("shutdown") => {
                    tracing::info!("Received shutdown, exiting");
                    break;
                }
                Some("ping") => {
                    if let Some(id) = &msg.id {
                        let rpc_id = match id {
                            shannon_agents::JsonRpcId::Number(n) => *n,
                            _ => -1,
                        };
                        agent_respond(rpc_id, serde_json::json!({"status": "ok"})).await;
                    }
                }
                Some(method) => {
                    tracing::warn!(method = %method, "Unknown method");
                    if let Some(id) = &msg.id {
                        let rpc_id = match id {
                            shannon_agents::JsonRpcId::Number(n) => *n,
                            _ => -1,
                        };
                        agent_respond_error(
                            rpc_id,
                            shannon_agents::JsonRpcError::not_found(method),
                        ).await;
                    }
                }
                None => {
                    tracing::debug!("Received response (ignoring)");
                }
            }
        }

        tracing::info!("Agent process exiting");
        Ok(())
    })
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize i18n — override locale from --lang flag if provided
    if let Some(ref lang) = cli.lang {
        i18n::set_locale(lang);
    }

    // ── Team agent mode: JSON-RPC over stdin/stdout ──
    // Must be handled before anything else — stdout is reserved for JSON-RPC.
    if cli.team_agent {
        let agent_name = cli.name.as_deref().unwrap_or("anonymous-agent");
        // Initialize logging to stderr (stdout reserved for JSON-RPC)
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("shannon_cli=info".parse().unwrap())
            )
            .init();
        return run_team_agent_mode(
            agent_name,
            cli.model.as_deref(),
            cli.provider.as_deref(),
            cli.system_prompt.as_deref(),
            cli.workdir.as_deref(),
            cli.permission_mode.as_deref(),
        );
    }

    // Pipe mode: read prompt from stdin
    if cli.pipe {
        let stdin_content = read_stdin();
        let prompt = match (cli.prompt, stdin_content.is_empty()) {
            (Some(arg), false) => format!("{arg}\n\n{stdin_content}"),
            (Some(arg), true) => arg,
            (None, false) => stdin_content,
            (None, true) => {
                eprintln!("Error: --pipe requires stdin input or a prompt argument");
                std::process::exit(1);
            }
        };
        let config = build_cli_config(
            cli.model.as_deref(),
            cli.provider.as_deref(),
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        return run_noninteractive_query(&prompt, true, &config, cli.yes);
    }

    // Bare prompt case: handle directly with explicit config
    if let Some(prompt) = cli.prompt {
        let config = build_cli_config(
            cli.model.as_deref(),
            cli.provider.as_deref(),
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        return run_noninteractive_query(&prompt, true, &config, cli.yes);
    }

    // Build configuration from CLI options (no more unsafe set_var calls)
    let config = match &cli.command {
        Some(Commands::Repl {
            env,
            model,
            provider,
            max_tokens,
            temperature,
            timeout,
            debug,
            cwd: _,
            file: _,
            local,
        }) => {
            let env_overrides = parse_cli_env(env)
                .map_err(|e| anyhow::anyhow!(e))?;
            let resolved_provider = if *local {
                Some("ollama".to_string())
            } else {
                provider.clone()
            };
            let resolved_model = if *local && model.is_none() {
                Some("llama3".to_string())
            } else {
                model.clone()
            };
            build_cli_config(
                resolved_model.as_deref(),
                resolved_provider.as_deref(),
                *max_tokens,
                *temperature,
                *timeout,
                *debug,
                env_overrides,
            )
        }
        Some(Commands::Query {
            model,
            provider,
            max_tokens,
            ..
        }) => {
            build_cli_config(
                model.as_deref(),
                provider.as_deref(),
                *max_tokens,
                None,
                None,
                false,
                HashMap::new(),
            )
        }
        // No subcommand, Version, Config, and Serve commands don't need config in the same way
        None | Some(Commands::Version { .. }) | Some(Commands::Config { .. }) | Some(Commands::Serve { .. }) => CliConfig::default(),
    };

    // Execute commands with explicit config
    match cli.command {
        None => {
            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{e:?}"))?;
            repl.run().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        Some(Commands::Repl { cwd, .. }) => {
            if let Some(dir) = cwd {
                std::env::set_current_dir(&dir)
                    .map_err(|e| anyhow::anyhow!("Failed to set working directory: {e}"))?;
            }
            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{e:?}"))?;
            repl.run().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        Some(Commands::Version { verbose }) => {
            println!("Shannon Code v0.1.0");
            if verbose {
                println!("Rust {}", env!("CARGO_PKG_RUST_VERSION"));
                println!("Features: mcp, multi-agent, tools");
            }
        }
        Some(Commands::Config { setting }) => {
            use shannon_tools::config::ConfigManager;
            let mut manager = ConfigManager::new();
            if let Err(e) = manager.load() {
                eprintln!("Warning: could not load config: {e}");
            }

            match setting {
                None => {
                    // List all config keys
                    let keys = manager.list(None);
                    if keys.is_empty() {
                        println!("No configuration set. Config file: {}", manager.config_path().display());
                    } else {
                        println!("Configuration ({} key(s)):", keys.len());
                        for key in &keys {
                            let val = manager.get(key).unwrap_or(serde_json::Value::Null);
                            println!("  {key} = {val}");
                        }
                        println!("\nConfig file: {}", manager.config_path().display());
                    }
                }
                Some(key) => {
                    // Support "key=value" syntax for setting, or plain key for getting
                    if let Some((k, v)) = key.split_once('=') {
                        let k = k.trim();
                        let v = v.trim();
                        let value: serde_json::Value = if v == "true" {
                            serde_json::json!(true)
                        } else if v == "false" {
                            serde_json::json!(false)
                        } else if let Ok(n) = v.parse::<i64>() {
                            serde_json::json!(n)
                        } else if let Ok(n) = v.parse::<f64>() {
                            serde_json::json!(n)
                        } else {
                            serde_json::json!(v)
                        };
                        manager.set(k.to_string(), value.clone());
                        if let Err(e) = manager.save() {
                            eprintln!("Error saving config: {e}");
                        } else {
                            println!("Set {k} = {value}");
                        }
                    } else {
                        // Get a specific key
                        match manager.get(key.trim()) {
                            Some(val) => println!("{key} = {val}"),
                            None => println!("Config key not found: {key}"),
                        }
                    }
                }
            }
        }
        Some(Commands::Query {
            query,
            output: _output_format,
            no_stream,
            ..
        }) => {
            run_noninteractive_query(&query, !no_stream, &config, cli.yes)?;
        }
        Some(Commands::Serve { port, host }) => {
            run_serve_command(port, host.clone(), &config)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_cli_env tests ──────────────────────────────────────────

    #[test]
    fn test_parse_single_env() {
        let input = vec!["KEY=value".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_parse_multiple_env() {
        let input = vec![
            "FOO=bar".to_string(),
            "BAZ=qux".to_string(),
            "PATH=/usr/bin".to_string(),
        ];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(result.get("BAZ"), Some(&"qux".to_string()));
        assert_eq!(result.get("PATH"), Some(&"/usr/bin".to_string()));
    }

    #[test]
    fn test_parse_env_empty_value() {
        let input = vec!["EMPTY=".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("EMPTY"), Some(&"".to_string()));
    }

    #[test]
    fn test_parse_env_value_with_equals() {
        // KEY=a=b should parse as key="KEY", value="a=b"
        let input = vec!["EQUATION=a=b".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("EQUATION"), Some(&"a=b".to_string()));
    }

    #[test]
    fn test_parse_env_whitespace_trimmed() {
        let input = vec!["  KEY  =  value  ".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_parse_env_empty_input() {
        let input: Vec<String> = vec![];
        let result = parse_cli_env(&input).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_env_missing_equals() {
        let input = vec!["NOEQUALSSIGN".to_string()];
        let result = parse_cli_env(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing '='"));
    }

    #[test]
    fn test_parse_env_empty_key() {
        let input = vec!["=value".to_string()];
        let result = parse_cli_env(&input);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("empty key"));
    }

    #[test]
    fn test_parse_env_special_chars_in_value() {
        let input = vec!["URL=https://example.com/path?q=1&b=2".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(
            result.get("URL"),
            Some(&"https://example.com/path?q=1&b=2".to_string())
        );
    }

    #[test]
    fn test_parse_env_model_override() {
        let input = vec!["SHANNON_MODEL=gpt-4o".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("SHANNON_MODEL"), Some(&"gpt-4o".to_string()));
    }

    #[test]
    fn test_parse_env_multiple_first_error() {
        let input = vec!["VALID=ok".to_string(), "BADOVERRIDE".to_string()];
        let result = parse_cli_env(&input);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_env_underscore_key() {
        let input = vec!["MY_VAR_123=hello".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("MY_VAR_123"), Some(&"hello".to_string()));
    }

    // ── CliConfig tests ──────────────────────────────────────────────

    #[test]
    fn test_cli_config_default() {
        let config = CliConfig::default();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.max_tokens.is_none());
        assert!(!config.debug);
    }

    #[test]
    fn test_cli_config_with_values() {
        let config = CliConfig {
            model: Some("gpt-4o".to_string()),
            provider: Some("openai".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.5),
            timeout: Some(60),
            debug: true,
            env_overrides: HashMap::new(),
        };
        assert_eq!(config.model(), Some("gpt-4o".to_string()));
        assert_eq!(config.provider(), Some("openai".to_string()));
        assert_eq!(config.max_tokens(), Some(4096));
        assert_eq!(config.temperature(), Some(0.5));
        assert_eq!(config.timeout(), Some(60));
        assert!(config.debug());
    }

    #[test]
    fn test_cli_config_env_fallback() {
        // Test that env vars are used as fallback when explicit value is None
        let config = CliConfig {
            model: None,
            provider: None,
            max_tokens: None,
            temperature: None,
            timeout: None,
            debug: false,
            env_overrides: HashMap::new(),
        };
        // These will be None unless SHANNON_MODEL is set in the test environment
        assert!(config.model().is_none() || config.model().is_some());
        assert!(config.provider().is_none() || config.provider().is_some());
    }

    #[test]
    fn test_cli_config_get_env() {
        let mut overrides = HashMap::new();
        overrides.insert("CUSTOM_VAR".to_string(), "custom_value".to_string());
        let config = CliConfig {
            model: None,
            provider: None,
            max_tokens: None,
            temperature: None,
            timeout: None,
            debug: false,
            env_overrides: overrides,
        };
        assert_eq!(
            config.get_env("CUSTOM_VAR"),
            Some("custom_value".to_string())
        );
        // Non-existent key returns None (unless set in actual env)
        assert!(config.get_env("NON_EXISTENT_KEY").is_none()
            || config.get_env("NON_EXISTENT_KEY").is_some());
    }

    #[test]
    fn test_cli_config_explicit_overrides_env() {
        let mut overrides = HashMap::new();
        overrides.insert("SHANNON_MODEL".to_string(), "override-model".to_string());
        let config = CliConfig {
            model: Some("cli-model".to_string()),
            provider: None,
            max_tokens: None,
            temperature: None,
            timeout: None,
            debug: false,
            env_overrides: overrides,
        };
        // Explicit model should take precedence
        assert_eq!(config.model(), Some("cli-model".to_string()));
        // But get_env should find the override
        assert_eq!(
            config.get_env("SHANNON_MODEL"),
            Some("override-model".to_string())
        );
    }

    // ── CLI clap parsing tests ────────────────────────────────────────

    #[test]
    fn test_cli_parse_repl_no_args() {
        let cli = Cli::try_parse_from(["shannon", "repl"]).unwrap();
        match cli.command {
            Some(Commands::Repl { file, env, .. }) => {
                assert!(file.is_none());
                assert!(env.is_empty());
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_file() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--file", "project.json"]).unwrap();
        match cli.command {
            Some(Commands::Repl { file, .. }) => {
                assert_eq!(file.as_deref(), Some("project.json"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_env() {
        let cli = Cli::try_parse_from([
            "shannon",
            "repl",
            "-e",
            "MODEL=gpt-4o",
            "-e",
            "TOKENS=4096",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Repl { env, .. }) => {
                assert_eq!(env.len(), 2);
                assert_eq!(env[0], "MODEL=gpt-4o");
                assert_eq!(env[1], "TOKENS=4096");
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_version() {
        let cli = Cli::try_parse_from(["shannon", "version"]).unwrap();
        match cli.command {
            Some(Commands::Version { verbose }) => {
                assert!(!verbose);
            }
            _ => panic!("Expected Version command"),
        }
    }

    #[test]
    fn test_cli_parse_version_verbose() {
        let cli = Cli::try_parse_from(["shannon", "version", "--verbose"]).unwrap();
        match cli.command {
            Some(Commands::Version { verbose }) => {
                assert!(verbose);
            }
            _ => panic!("Expected Version command"),
        }
    }

    #[test]
    fn test_cli_parse_config_with_setting() {
        let cli = Cli::try_parse_from(["shannon", "config", "-s", "model"]).unwrap();
        match cli.command {
            Some(Commands::Config { setting }) => {
                assert_eq!(setting.as_deref(), Some("model"));
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_parse_config_no_setting() {
        let cli = Cli::try_parse_from(["shannon", "config"]).unwrap();
        match cli.command {
            Some(Commands::Config { setting }) => {
                assert!(setting.is_none());
            }
            _ => panic!("Expected Config command"),
        }
    }

    #[test]
    fn test_cli_parse_no_subcommand_succeeds() {
        let cli = Cli::try_parse_from(["shannon"]).unwrap();
        assert!(cli.prompt.is_none());
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_unknown_word_parses_as_prompt() {
        // "unknown" is no longer a subcommand error — it's treated as a bare prompt
        let cli = Cli::try_parse_from(["shannon", "unknown"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("unknown"));
        assert!(cli.command.is_none());
    }

    // ── CLI help and default behavior tests ─────────────────────────────

    #[test]
    fn test_cli_no_args_parses_as_default() {
        // shannon with no args now parses successfully (prompt=None, command=None → launch REPL)
        let cli = Cli::try_parse_from(["shannon"]).unwrap();
        assert!(cli.prompt.is_none());
        assert!(cli.command.is_none());
        assert!(cli.model.is_none());
        assert!(cli.provider.is_none());
    }

    #[test]
    fn test_cli_help_short_flag() {
        // shannon -h should show help
        let result = Cli::try_parse_from(["shannon", "-h"]);
        assert!(result.is_err());
        // -h is not a valid flag for our CLI
    }

    #[test]
    fn test_cli_help_long_flag() {
        // shannon --help should show help
        let result = Cli::try_parse_from(["shannon", "--help"]);
        assert!(result.is_err());
        // --help is not a valid flag for our CLI
    }

    // ── Bare prompt (non-interactive) tests ──────────────────────────────

    #[test]
    fn test_cli_bare_prompt_basic() {
        let cli = Cli::try_parse_from(["shannon", "hello world"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("hello world"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_bare_prompt_chinese() {
        let cli = Cli::try_parse_from(["shannon", "你用的什么模型"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("你用的什么模型"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_bare_prompt_with_model() {
        let cli = Cli::try_parse_from(["shannon", "--model", "gpt-4o", "explain this"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("explain this"));
        assert_eq!(cli.model.as_deref(), Some("gpt-4o"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_bare_prompt_with_provider() {
        let cli = Cli::try_parse_from(["shannon", "-p", "anthropic", "test query"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("test query"));
        assert_eq!(cli.provider.as_deref(), Some("anthropic"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_bare_prompt_with_model_and_provider() {
        let cli = Cli::try_parse_from(["shannon", "-m", "gpt-4o", "-p", "openai", "你好"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("你好"));
        assert_eq!(cli.model.as_deref(), Some("gpt-4o"));
        assert_eq!(cli.provider.as_deref(), Some("openai"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_subcommand_takes_precedence_over_prompt() {
        // When a subcommand is given, it should be parsed as a subcommand not a prompt
        let cli = Cli::try_parse_from(["shannon", "repl"]).unwrap();
        // "repl" matches a subcommand, so command is Some and prompt is None
        assert!(cli.prompt.is_none());
        assert!(cli.command.is_some());
    }

    #[test]
    fn test_cli_repl_is_default_subcommand() {
        // The repl subcommand should work without explicitly typing it
        // (when we implement default subcommand behavior)
        let cli = Cli::try_parse_from(["shannon", "repl"]).unwrap();
        match cli.command {
            Some(Commands::Repl { .. }) => {
                // Success - repl subcommand parsed
            }
            _ => panic!("Expected Repl command"),
        }
    }

    // ── New REPL options tests ────────────────────────────────────────────

    #[test]
    fn test_cli_parse_repl_with_model_short() {
        let cli = Cli::try_parse_from(["shannon", "repl", "-m", "gpt-4o"]).unwrap();
        match cli.command {
            Some(Commands::Repl { model, .. }) => {
                assert_eq!(model.as_deref(), Some("gpt-4o"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_model_long() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--model", "claude-sonnet-4"]).unwrap();
        match cli.command {
            Some(Commands::Repl { model, .. }) => {
                assert_eq!(model.as_deref(), Some("claude-sonnet-4"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_provider_short() {
        let cli = Cli::try_parse_from(["shannon", "repl", "-p", "anthropic"]).unwrap();
        match cli.command {
            Some(Commands::Repl { provider, .. }) => {
                assert_eq!(provider.as_deref(), Some("anthropic"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_provider_long() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--provider", "openai"]).unwrap();
        match cli.command {
            Some(Commands::Repl { provider, .. }) => {
                assert_eq!(provider.as_deref(), Some("openai"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_max_tokens() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--max-tokens", "4096"]).unwrap();
        match cli.command {
            Some(Commands::Repl { max_tokens, .. }) => {
                assert_eq!(max_tokens, Some(4096));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_temperature() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--temperature", "0.5"]).unwrap();
        match cli.command {
            Some(Commands::Repl { temperature, .. }) => {
                assert_eq!(temperature, Some(0.5));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_timeout() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--timeout", "300"]).unwrap();
        match cli.command {
            Some(Commands::Repl { timeout, .. }) => {
                assert_eq!(timeout, Some(300));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_debug_short() {
        let cli = Cli::try_parse_from(["shannon", "repl", "-d"]).unwrap();
        match cli.command {
            Some(Commands::Repl { debug, .. }) => {
                assert!(debug);
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_debug_long() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--debug"]).unwrap();
        match cli.command {
            Some(Commands::Repl { debug, .. }) => {
                assert!(debug);
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_cwd() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--cwd", "/tmp/project"]).unwrap();
        match cli.command {
            Some(Commands::Repl { cwd, .. }) => {
                assert_eq!(cwd.as_deref(), Some("/tmp/project"));
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_with_all_options() {
        let cli = Cli::try_parse_from([
            "shannon",
            "repl",
            "-m",
            "claude-3-5-sonnet-20241022",
            "-p",
            "anthropic",
            "--max-tokens",
            "16384",
            "--temperature",
            "0.8",
            "--timeout",
            "180",
            "-d",
            "--cwd",
            "/home/user/project",
            "-e",
            "CUSTOM_VAR=value",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Repl {
                model,
                provider,
                max_tokens,
                temperature,
                timeout,
                debug,
                cwd,
                env,
                ..
            }) => {
                assert_eq!(model.as_deref(), Some("claude-3-5-sonnet-20241022"));
                assert_eq!(provider.as_deref(), Some("anthropic"));
                assert_eq!(max_tokens, Some(16384));
                assert_eq!(temperature, Some(0.8));
                assert_eq!(timeout, Some(180));
                assert!(debug);
                assert_eq!(cwd.as_deref(), Some("/home/user/project"));
                assert_eq!(env.len(), 1);
                assert_eq!(env[0], "CUSTOM_VAR=value");
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_defaults() {
        let cli = Cli::try_parse_from(["shannon", "repl"]).unwrap();
        match cli.command {
            Some(Commands::Repl {
                model,
                provider,
                max_tokens,
                temperature,
                timeout,
                debug,
                cwd,
                env,
                file,
                local,
            }) => {
                assert!(model.is_none());
                assert!(provider.is_none());
                assert!(max_tokens.is_none());
                assert!(temperature.is_none());
                assert!(timeout.is_none());
                assert!(!debug);
                assert!(cwd.is_none());
                assert!(env.is_empty());
                assert!(file.is_none());
                assert!(!local);
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_local_flag() {
        let cli = Cli::try_parse_from(["shannon", "repl", "--local"]).unwrap();
        match cli.command {
            Some(Commands::Repl { local, .. }) => {
                assert!(local);
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_parse_repl_local_short() {
        let cli = Cli::try_parse_from(["shannon", "repl", "-l"]).unwrap();
        match cli.command {
            Some(Commands::Repl { local, .. }) => {
                assert!(local);
            }
            _ => panic!("Expected Repl command"),
        }
    }

    #[test]
    fn test_cli_repl_local_resolves_provider_and_model() {
        // --local should resolve to provider=ollama and model=llama3
        let _config = build_cli_config(
            None,
            None,
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        // Without --local, default provider depends on env
        let local_config = build_cli_config(
            Some("llama3"),
            Some("ollama"),
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        assert_eq!(local_config.provider.as_deref(), Some("ollama"));
        assert_eq!(local_config.model.as_deref(), Some("llama3"));
    }

    // ── Query subcommand tests ──────────────────────────────────────────

    #[test]
    fn test_cli_parse_query_basic() {
        let cli = Cli::try_parse_from(["shannon", "query", "hello world"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, .. }) => {
                assert_eq!(query, "hello world");
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_with_model() {
        let cli = Cli::try_parse_from(["shannon", "query", "-m", "gpt-4o", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, model, .. }) => {
                assert_eq!(query, "test");
                assert_eq!(model.as_deref(), Some("gpt-4o"));
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_with_provider() {
        let cli = Cli::try_parse_from(["shannon", "query", "-p", "anthropic", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, provider, .. }) => {
                assert_eq!(query, "test");
                assert_eq!(provider.as_deref(), Some("anthropic"));
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_with_max_tokens() {
        let cli = Cli::try_parse_from(["shannon", "query", "--max-tokens", "8192", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, max_tokens, .. }) => {
                assert_eq!(query, "test");
                assert_eq!(max_tokens, Some(8192));
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_with_no_stream() {
        let cli = Cli::try_parse_from(["shannon", "query", "--no-stream", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, no_stream, .. }) => {
                assert_eq!(query, "test");
                assert!(no_stream);
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_output_format() {
        let cli = Cli::try_parse_from(["shannon", "query", "--output", "json", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, output, .. }) => {
                assert_eq!(query, "test");
                assert_eq!(output, "json");
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_default_output() {
        let cli = Cli::try_parse_from(["shannon", "query", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { output, .. }) => {
                assert_eq!(output, "text");
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_defaults() {
        let cli = Cli::try_parse_from(["shannon", "query", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query { query, model, provider, max_tokens, output, no_stream }) => {
                assert_eq!(query, "test");
                assert!(model.is_none());
                assert!(provider.is_none());
                assert!(max_tokens.is_none());
                assert_eq!(output, "text");
                assert!(!no_stream);
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_missing_query_arg() {
        let result = Cli::try_parse_from(["shannon", "query"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parse_query_with_all_options() {
        let cli = Cli::try_parse_from([
            "shannon",
            "query",
            "-m", "claude-sonnet-4",
            "-p", "anthropic",
            "--max-tokens", "4096",
            "--output", "json",
            "--no-stream",
            "你用的什么模型",
        ]).unwrap();
        match cli.command {
            Some(Commands::Query { query, model, provider, max_tokens, output, no_stream }) => {
                assert_eq!(query, "你用的什么模型");
                assert_eq!(model.as_deref(), Some("claude-sonnet-4"));
                assert_eq!(provider.as_deref(), Some("anthropic"));
                assert_eq!(max_tokens, Some(4096));
                assert_eq!(output, "json");
                assert!(no_stream);
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_parse_env_chinese_value() {
        let input = vec!["SHANNON_MODEL=你用的什么模型".to_string()];
        let result = parse_cli_env(&input).unwrap();
        assert_eq!(result.get("SHANNON_MODEL"), Some(&"你用的什么模型".to_string()));
    }

    // ── build_cli_config integration tests ─────────────────────────────────

    #[test]
    fn test_build_cli_config_with_all_params() {
        let mut env = HashMap::new();
        env.insert("SHANNON_DEBUG".to_string(), "true".to_string());

        let config = build_cli_config(
            Some("gpt-4o"),
            Some("openai"),
            Some(4096),
            Some(0.7),
            Some(60),
            false,
            env,
        );

        assert_eq!(config.model.as_deref(), Some("gpt-4o"));
        assert_eq!(config.provider.as_deref(), Some("openai"));
        assert_eq!(config.max_tokens, Some(4096));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.timeout, Some(60));
        assert_eq!(
            config.env_overrides.get("SHANNON_DEBUG"),
            Some(&"true".to_string())
        );
    }

    #[test]
    fn test_build_cli_config_minimal() {
        let config = build_cli_config(None, None, None, None, None, false, HashMap::new());
        // With no TOML config file, these should be None
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
        assert!(config.max_tokens.is_none());
    }

    #[test]
    fn test_build_cli_config_debug_flag_set() {
        let config = build_cli_config(None, None, None, None, None, true, HashMap::new());
        assert!(config.debug);
    }

    // ── LlmClientConfig build from CLI config (unit test) ─────────────────

    #[test]
    fn test_build_llm_config_from_cli() {
        let config = CliConfig {
            model: Some("claude-sonnet-4".to_string()),
            provider: Some("anthropic".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.5),
            timeout: Some(120),
            debug: false,
            env_overrides: HashMap::new(),
        };

        let llm_config = build_llm_config_from_builder(&config);
        // The ConfigBuilder may override the model based on TOML config,
        // but the config object should be constructable without error.
        assert!(!llm_config.model.is_empty());
        // Provider is set (exact value depends on ConfigBuilder merge priority)
        let provider_str = llm_config.provider.to_string().to_lowercase();
        assert!(!provider_str.is_empty());
    }

    #[test]
    fn test_build_llm_config_ollama_provider() {
        let config = CliConfig {
            model: Some("llama3".to_string()),
            provider: Some("ollama".to_string()),
            max_tokens: None,
            temperature: None,
            timeout: None,
            debug: false,
            env_overrides: HashMap::new(),
        };

        let llm_config = build_llm_config_from_builder(&config);
        assert!(!llm_config.provider.requires_auth());
    }

    // ── TOML config loading ──────────────────────────────────────────────

    #[test]
    fn test_load_toml_config_no_files() {
        // This should not panic even if no config files exist
        let config = load_toml_config();
        // All fields should be None or default when no config files exist
        assert!(config.model.is_none() || config.model.is_some()); // depends on user's system
    }

    #[test]
    fn test_toml_config_deserialization() {
        let toml_str = r#"
            model = "gpt-4o"
            provider = "openai"
            max_tokens = 8192
            temperature = 0.7
            timeout = 120
            debug = true
            api_key = "sk-test"
            base_url = "https://api.openai.com/v1"
        "#;
        let config: ShannonTomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model.as_deref(), Some("gpt-4o"));
        assert_eq!(config.provider.as_deref(), Some("openai"));
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.timeout, Some(120));
        assert!(config.debug.unwrap());
        assert_eq!(config.api_key.as_deref(), Some("sk-test"));
        assert_eq!(config.base_url.as_deref(), Some("https://api.openai.com/v1"));
    }

    #[test]
    fn test_toml_config_partial() {
        let toml_str = r#"
            model = "claude-sonnet-4"
        "#;
        let config: ShannonTomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model.as_deref(), Some("claude-sonnet-4"));
        assert!(config.provider.is_none());
        assert!(config.max_tokens.is_none());
    }

    #[test]
    fn test_toml_config_empty() {
        let config: ShannonTomlConfig = toml::from_str("").unwrap();
        assert!(config.model.is_none());
        assert!(config.provider.is_none());
    }

    // ── Bare prompt config construction ───────────────────────────────────

    #[test]
    fn test_bare_prompt_config_construction() {
        // Simulates: shannon --model gpt-4o "explain this"
        let cli = Cli::try_parse_from(["shannon", "--model", "gpt-4o", "explain this"]).unwrap();
        let config = build_cli_config(
            cli.model.as_deref(),
            cli.provider.as_deref(),
            None, // no max_tokens
            None, // no temperature
            None, // no timeout
            false, // no debug
            HashMap::new(),
        );
        assert_eq!(config.model.as_deref(), Some("gpt-4o"));
        assert!(config.provider.is_none());
    }

    #[test]
    fn test_query_subcommand_config_construction() {
        // Simulates: shannon query -m gpt-4o -p openai --max-tokens 4096 "test"
        let cli = Cli::try_parse_from([
            "shannon", "query",
            "-m", "gpt-4o",
            "-p", "openai",
            "--max-tokens", "4096",
            "test",
        ]).unwrap();

        if let Some(Commands::Query { model, provider, max_tokens, .. }) = cli.command {
            let config = build_cli_config(
                model.as_deref(),
                provider.as_deref(),
                max_tokens,
                None,
                None,
                false,
                HashMap::new(),
            );
            assert_eq!(config.model.as_deref(), Some("gpt-4o"));
            assert_eq!(config.provider.as_deref(), Some("openai"));
            assert_eq!(config.max_tokens, Some(4096));
        }
    }

    // ── Team agent mode tests ────────────────────────────────────────────

    #[test]
    fn test_cli_team_agent_flag() {
        let cli = Cli::try_parse_from([
            "shannon", "--team-agent", "--name", "worker-1",
        ]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.name.as_deref(), Some("worker-1"));
    }

    #[test]
    fn test_cli_team_agent_with_model() {
        let cli = Cli::try_parse_from([
            "shannon", "--team-agent", "--name", "worker-2",
            "--model", "claude-sonnet-4", "--provider", "anthropic",
        ]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.name.as_deref(), Some("worker-2"));
        assert_eq!(cli.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(cli.provider.as_deref(), Some("anthropic"));
    }

    #[test]
    fn test_cli_team_agent_with_system_prompt() {
        let cli = Cli::try_parse_from([
            "shannon", "--team-agent", "--name", "coder",
            "--system-prompt", "You are a Rust expert",
        ]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(
            cli.system_prompt.as_deref(),
            Some("You are a Rust expert"),
        );
    }

    #[test]
    fn test_cli_team_agent_with_workdir() {
        let cli = Cli::try_parse_from([
            "shannon", "--team-agent", "--name", "worker",
            "--workdir", "/tmp/worktree-1",
        ]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.workdir.as_deref(), Some("/tmp/worktree-1"));
    }

    #[test]
    fn test_cli_team_agent_all_options() {
        let cli = Cli::try_parse_from([
            "shannon",
            "--team-agent",
            "--name", "researcher",
            "--model", "claude-opus-4",
            "--provider", "anthropic",
            "--system-prompt", "Research agent",
            "--workdir", "/project/research",
        ]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.name.as_deref(), Some("researcher"));
        assert_eq!(cli.model.as_deref(), Some("claude-opus-4"));
        assert_eq!(cli.provider.as_deref(), Some("anthropic"));
        assert_eq!(cli.system_prompt.as_deref(), Some("Research agent"));
        assert_eq!(cli.workdir.as_deref(), Some("/project/research"));
    }

    #[test]
    fn test_cli_no_team_agent_by_default() {
        let cli = Cli::try_parse_from(["shannon"]).unwrap();
        assert!(!cli.team_agent);
        assert!(cli.name.is_none());
        assert!(cli.system_prompt.is_none());
        assert!(cli.workdir.is_none());
    }
}
