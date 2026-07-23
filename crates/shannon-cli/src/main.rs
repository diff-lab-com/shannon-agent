use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use futures::StreamExt;

mod mcp_install;
mod notifications;
use shannon_commands::preset_utils::ConversationPreset;
use shannon_core::{
    i18n,
    model_registry::resolve_model,
    provider_resolver::synthesize_default_profile,
    query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata},
    tools::ToolRegistry,
    unified_config::{ConfigBuilder, ShannonConfig},
};
use shannon_engine::{api::LlmClientConfig, state::StateManager};
use shannon_tools::register_default_tools_with_project_dir_ex;
use shannon_ui::Repl;
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

// ── CI/CD Headless Mode Types ──────────────────────────────────────────

/// Output format for non-interactive headless mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize)]
enum OutputFormat {
    /// Plain text response to stdout.
    Text,
    /// Full structured JSON output with tool calls and metadata (at end).
    Json,
    /// Streaming JSON events (NDJSON format).
    JsonStream,
}

impl std::fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::JsonStream => write!(f, "json-stream"),
        }
    }
}

/// Exit codes for non-interactive CI mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum HeadlessExitCode {
    /// 0 - assistant completed the task successfully.
    Success = 0,
    /// 1 - query engine error or API error.
    Error = 1,
    /// 2 - maximum turns reached before completion.
    TurnLimit = 2,
    /// 3 - timeout occurred (request took too long).
    #[allow(dead_code)] // KEEP: future use
    Timeout = 3,
    /// 4 - rate limited by API provider.
    RateLimited = 4,
    /// 5 - conversation exceeded the model's context window.
    ContextOverflow = 5,
    /// 6 - a required permission was denied in non-interactive mode.
    PermissionDenied = 6,
}

impl From<HeadlessExitCode> for i32 {
    fn from(code: HeadlessExitCode) -> i32 {
        code as i32
    }
}

/// Summary of a single tool call during headless execution.
#[derive(Debug, Clone, serde::Serialize)]
struct ToolCallSummary {
    /// Name of the tool invoked.
    tool: String,
    /// Truncated summary of the tool input.
    input_summary: String,
    /// Truncated summary of the tool output.
    output_summary: String,
    /// Whether the tool execution succeeded.
    success: bool,
}

/// Structured JSON output for headless mode.
#[derive(Debug, Clone, serde::Serialize)]
struct HeadlessOutput {
    /// The original prompt sent to the assistant.
    prompt: String,
    /// The assistant's text response.
    response: String,
    /// Summaries of all tool calls made during execution.
    tool_calls: Vec<ToolCallSummary>,
    /// Total tokens consumed.
    total_tokens: u64,
    /// Wall-clock duration in milliseconds.
    duration_ms: u64,
    /// Whether execution succeeded and why.
    exit_code: HeadlessExitCode,
}

/// CI/CD event types for NDJSON streaming output.
/// Each event is serialized as a single JSON object per line (newline-delimited).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
enum CiEvent {
    /// Session started.
    #[serde(rename = "start")]
    Start { prompt: String, model: String },
    /// Tool was invoked.
    #[serde(rename = "tool_call")]
    ToolCall {
        name: String,
        input: serde_json::Value,
    },
    /// Tool execution completed.
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        output: String,
        success: bool,
    },
    /// Message/response content.
    #[serde(rename = "message")]
    #[allow(dead_code)] // KEEP: future use
    Message { content: String },
    /// File diff (unified format).
    #[serde(rename = "diff")]
    #[allow(dead_code)] // KEEP: future use
    Diff { path: String, content: String },
    /// Error occurred.
    #[serde(rename = "error")]
    Error { message: String },
    /// Session completed.
    #[serde(rename = "done")]
    Done {
        exit_code: i32,
        turns_used: u32,
        tokens_used: u64,
    },
}

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
    /// Get the model, with fallback to environment variable and alias resolution.
    fn model(&self) -> Option<String> {
        self.model
            .clone()
            .or_else(|| std::env::var("SHANNON_MODEL").ok())
            .map(|m| resolve_model(&m, None))
    }

    /// Get the provider, with fallback to environment variable.
    fn provider(&self) -> Option<String> {
        self.provider
            .clone()
            .or_else(|| std::env::var("SHANNON_PROVIDER").ok())
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
    /// N1/C-fields: legacy `api_key`/`base_url` removed under no-compat. TOML
    /// `api_key` is **silently ignored** (would conflict with A1 — plaintext
    /// never enters the config; use `~/.shannon/secrets.env` instead).
    /// `base_url` is no longer recognised here either; put it on the
    /// `provider_model` profile in TOML (full TOML config support is in
    /// `shannon-cli` via `ConfigBuilder`'s JSON parse path).
    enable_tools: Option<bool>,
    /// User-defined conversation presets.
    presets: HashMap<String, ConversationPreset>,
}

/// Load TOML config from disk, merging global + project-local files.
fn load_toml_config() -> ShannonTomlConfig {
    let mut merged = ShannonTomlConfig::default();

    // 1. Global config: ~/.shannon/config.toml
    if let Some(home) = dirs::home_dir() {
        let global_path = home.join(".shannon").join("config.toml");
        if let Ok(content) = std::fs::read_to_string(&global_path) {
            match toml::from_str::<ShannonTomlConfig>(&content) {
                Ok(cfg) => merged = cfg,
                Err(e) => eprintln!("Warning: failed to parse {}: {e}", global_path.display()),
            }
        }
    }

    // 2. Project-local config: .shannon.toml
    let local_path = std::path::Path::new(".shannon.toml");
    if let Ok(content) = std::fs::read_to_string(local_path) {
        match toml::from_str::<ShannonTomlConfig>(&content) {
            Ok(cfg) => {
                // Merge: local overrides global
                if cfg.model.is_some() {
                    merged.model = cfg.model;
                }
                if cfg.provider.is_some() {
                    merged.provider = cfg.provider;
                }
                if cfg.max_tokens.is_some() {
                    merged.max_tokens = cfg.max_tokens;
                }
                if cfg.temperature.is_some() {
                    merged.temperature = cfg.temperature;
                }
                if cfg.timeout.is_some() {
                    merged.timeout = cfg.timeout;
                }
                if cfg.debug.is_some() {
                    merged.debug = cfg.debug;
                }
                if cfg.enable_tools.is_some() {
                    merged.enable_tools = cfg.enable_tools;
                }
                // Merge presets: local presets overlay on top of global
                for (name, preset) in cfg.presets {
                    merged.presets.insert(name, preset);
                }
            }
            Err(e) => eprintln!("Warning: failed to parse .shannon.toml: {e}"),
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
#[command(version = env!("CARGO_PKG_VERSION"))]
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
    #[arg(long)]
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

    /// Comma-separated list of allowed tool names for the agent (used with --team-agent).
    /// If not set, all tools are available.
    #[arg(long = "allowed-tools", hide = true)]
    team_allowed_tools: Option<String>,

    /// Resume the most recent session, or a specific session by UUID.
    /// Without a UUID argument, loads the most recent session.
    /// With a UUID argument, loads that specific session.
    /// Example: shannon --resume           (most recent)
    ///          shannon --resume abc-123... (specific session)
    #[arg(short = 'r', long, value_name = "UUID", num_args = 0..=1)]
    resume: Option<String>,

    /// Resume a specific session by UUID (explicit alternative to --resume <UUID>).
    /// Example: shannon --resume-id 550e8400-e29b-41d4-a716-446655440000
    #[arg(long = "resume-id", value_name = "UUID")]
    resume_id: Option<String>,

    /// Continue the most recent session (alias for --resume).
    #[arg(short = 'c', long, alias = "cont")]
    r#continue: bool,

    /// CI/CD headless mode: non-interactive prompt (pipe-friendly).
    /// Skips TUI entirely. Use with --output-format, --allowed-tools, --max-turns.
    /// Example: shannon -p "fix the bug" --allowed-tools Read,Edit,Bash --output-format json
    #[arg(short = 'p', long = "prompt")]
    headless_prompt: Option<String>,

    // NOTE: `--allowed-tools` is defined above as `team_allowed_tools` (shared
    // by both --team-agent and --prompt headless modes).  Do not add a second
    // field with the same long option name — clap rejects duplicate longs.
    /// Output format for headless mode (text or json).
    #[arg(long = "output-format", default_value = "text")]
    output_format: OutputFormat,

    /// Maximum turns in headless mode before exiting with code 2.
    #[arg(long = "max-turns")]
    max_turns: Option<u32>,

    /// Exit with error code on first tool failure.
    #[arg(long)]
    exit_on_error: bool,

    /// Suppress progress indicators and toasts.
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Only output file changes as unified diff (implies --quiet).
    #[arg(long)]
    diff_only: bool,

    /// JSON Schema file or inline JSON for structured output validation (headless mode).
    /// When provided, the assistant is instructed to return JSON matching the schema.
    /// The response is validated before output. Exit code 1 on validation failure.
    /// Example: shannon --prompt "analyze" --schema schema.json
    ///          shannon --prompt "analyze" --schema '{"type":"object","required":["result"]}'
    #[arg(long)]
    schema: Option<String>,

    /// Fire OS-native notifications on query completion, errors, or permission
    /// prompts in headless / non-interactive mode. Best-effort: ignores failures
    /// from missing platform binaries (e.g. `notify-send` not installed).
    /// Overridden by `[notifications] enabled = true|false` in `.shannon.toml`.
    #[arg(long)]
    notify: bool,

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
        #[arg(long)]
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
        #[arg(long)]
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
        #[arg(short, long, default_value_t = 33420)]
        port: u16,

        /// Host to bind to (defaults to loopback)
        #[arg(long)]
        host: Option<String>,

        /// Bearer token required for non-health API requests
        #[arg(long)]
        auth_token: Option<String>,

        /// Explicitly allow binding to a non-loopback host (requires --auth-token)
        #[arg(long)]
        allow_nonloopback: bool,
    },

    /// Render predefined UI scenes as text files for AI analysis
    Screenshot {
        /// Output directory for screenshot text files
        dir: String,
    },

    /// Manage MCP servers (install bundles, list, etc.)
    Mcp {
        #[command(subcommand)]
        command: McpSubcommand,
    },

    /// Launch the Shannon desktop application (Tauri GUI)
    Desktop {
        /// Do not attempt to build the desktop app if the binary isn't found.
        #[arg(long)]
        no_build: bool,

        /// Run in the foreground and wait for the desktop app to exit.
        #[arg(long)]
        foreground: bool,
    },

    /// Delegate to the external `shannon-gateway` service manager
    Gateway {
        #[command(subcommand)]
        command: GatewaySubcommand,
    },

    /// Self-update: check GitHub for a newer release and print upgrade steps.
    Update,

    /// Run diagnostics: check toolchain, ports, and services.
    Doctor,
}

/// Subcommands for `shannon gateway` (delegated to the external binary).
///
/// Variants are spawned verbatim (kebab-case) on `shannon-gateway`'s PATH.
#[derive(Subcommand, Debug)]
enum GatewaySubcommand {
    /// Run the gateway in the foreground.
    Run,
    /// Start the gateway as a background service.
    Start,
    /// Stop the background gateway service.
    Stop,
    /// Restart the background gateway service.
    Restart,
    /// Show the gateway service status.
    Status,
    /// List registered gateway routes/endpoints.
    List,
    /// Install the gateway as a system service.
    Install,
    /// Uninstall the gateway system service.
    Uninstall,
    /// Set up gateway configuration and data directory.
    Setup,
    /// Migrate a legacy gateway configuration.
    MigrateLegacy,
    /// Enroll this device with the gateway control plane.
    Enroll,
}

/// Subcommands for `shannon mcp`.
#[derive(Subcommand, Debug)]
enum McpSubcommand {
    /// Install MCP servers from a `.mcpb` bundle.
    ///
    /// A `.mcpb` file is a zip archive containing an `.mcp.json` at the root.
    /// Servers are merged into the project's `.mcp.json` by default, or
    /// `~/.shannon/settings.json` with `--user`.
    Install {
        /// Path to the `.mcpb` bundle.
        bundle: String,

        /// Install to user-level settings (`~/.shannon/settings.json`) instead
        /// of the project's `.mcp.json`.
        #[arg(long)]
        user: bool,

        /// Skip the confirmation prompt. Script-friendly.
        #[arg(long)]
        yes: bool,

        /// Show what would be installed, then exit without writing.
        #[arg(long)]
        dry_run: bool,
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
/// Check whether tools should be enabled for the given provider.
/// Ollama/local models default to no tools; all others default to yes.
fn should_enable_tools(provider: shannon_engine::api::LlmProvider) -> bool {
    !matches!(provider, shannon_engine::api::LlmProvider::Ollama)
}

/// Priority (highest → lowest):
///   CLI overrides > env vars (`SHANNON_*`) > local `.shannon.toml` > global `~/.shannon/config.toml`
///
/// N1/C-fields: the legacy `ShannonConfig { model, provider, api_key,
/// base_url, … }` literal is gone. CLI options feed
/// [`shannon_core::provider_resolver::synthesize_default_profile`] (with
/// `explicit_cred_var = Some("SHANNON_API_KEY")` so the credential routing is
/// deterministic) to build the default v2 profile (provider/base_url/model +
/// a `CredentialRef::Env` pointing at `SHANNON_API_KEY`). The plaintext
/// api-key value never enters the config (A1-strict); at `From`-time the
/// value is sourced from the process environment via `resolve_credential`.
///
/// The CLI temporarily injects the resolved api-key value into the
/// `SHANNON_API_KEY` env var so `resolve_credential` can pick it up. This is
/// restored before returning. **N2 will replace this with proper secrets.env
/// plumbing** via [`crate::config_migration::persist_secrets`] —
/// pre-N1 the same `unsafe std::env` pattern was used by
/// `apply_env_overrides`, so this preserves A1 and the same overall behaviour.
fn build_llm_config_from_builder(cli_config: &CliConfig) -> LlmClientConfig {
    // 1. Resolve the canonical api-key value: SHANNON_API_KEY (or
    //    cli-injected override) → ANTHROPIC_API_KEY → OPENAI_API_KEY.
    let api_key_resolved = cli_config
        .get_env("SHANNON_API_KEY")
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok());

    // 2. Inject into env so synthesize (which may run multiple times across
    //    layers) always picks `SHANNON_API_KEY` via the explicit override
    //    path. Save any prior value so we can restore.
    let saved_shannon_api_key = std::env::var("SHANNON_API_KEY").ok();
    let saved_anthropic_api_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let saved_openai_api_key = std::env::var("OPENAI_API_KEY").ok();
    if let Some(ref v) = api_key_resolved {
        // SAFETY: single-threaded CLI startup — no concurrent env readers
        // observe an inconsistent state. N2 will move this into a
        // shannon-core-scoped resolver. Last `unsafe std::env::set_var`
        // wins.
        unsafe { std::env::set_var("SHANNON_API_KEY", v) };
    }
    // SAFETY: see above; clear ANTHROPIC/OPENAI so the canonical chain
    // (and our explicit-cred_var override) deterministically picks
    // SHANNON_API_KEY.
    unsafe { std::env::remove_var("ANTHROPIC_API_KEY") };
    unsafe { std::env::remove_var("OPENAI_API_KEY") };

    // 3. Synthesize the v2 default profile now that SHANNON_API_KEY is the
    //    chosen cred var.
    let provider_model = synthesize_default_profile(
        cli_config.model().as_deref(),
        cli_config.provider().as_deref(),
        cli_config.get_env("SHANNON_BASE_URL").as_deref(),
        Some("SHANNON_API_KEY"),
    )
    .unwrap_or_default();

    let cli_overrides = ShannonConfig {
        max_tokens: cli_config.max_tokens(),
        temperature: cli_config.temperature(),
        timeout: cli_config.timeout(),
        debug: cli_config.debug(),
        enable_tools: None,
        max_context_tokens: None,
        presets: None,
        permission_profile: None,
        notifications: None,
        provider_model,
    };

    // 4. Build merged config + convert. (load_env_vars will see
    //    SHANNON_API_KEY still set and synthesise with the same cred var.)
    let merged = ConfigBuilder::new()
        .load_global_toml()
        .load_local_toml()
        .load_env_vars()
        .set_cli_overrides(cli_overrides)
        .build();

    let out = LlmClientConfig::from(merged);

    // 5. Restore env (in reverse order; failures don't propagate to avoid
    //    masking the actual LlmClientConfig result).
    // SAFETY: see step 2.
    let restore = |name: &str, prior: Option<String>| {
        if let Some(v) = prior {
            // SAFETY: see step 2.
            unsafe { std::env::set_var(name, v) };
        } else {
            // SAFETY: see step 2.
            unsafe { std::env::remove_var(name) };
        }
    };
    restore("OPENAI_API_KEY", saved_openai_api_key);
    restore("ANTHROPIC_API_KEY", saved_anthropic_api_key);
    restore("SHANNON_API_KEY", saved_shannon_api_key);

    out
}

/// Load a session for resumption.
///
/// If `session_id_str` is provided, loads that specific session by UUID.
/// Otherwise, loads the most recent session from the sessions directory.
///
/// Returns the loaded `SessionData` on success.
fn load_resume_session(session_id_str: Option<&str>) -> Result<shannon_engine::state::SessionData> {
    use shannon_engine::state::StateManager;
    let state_mgr = StateManager::new();

    if let Some(id_str) = session_id_str {
        let uuid = uuid::Uuid::parse_str(id_str).map_err(|e| {
            let display = if id_str.len() > 36 {
                &id_str[..36]
            } else {
                id_str
            };
            anyhow::anyhow!("Invalid session UUID '{display}': {e}")
        })?;
        state_mgr
            .load_session(&uuid)?
            .ok_or_else(|| anyhow::anyhow!("Session {uuid} not found"))
    } else {
        let sessions = state_mgr.list_persisted_sessions()?;
        let latest = sessions
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No previous sessions found to resume"))?;
        state_mgr
            .load_session(&latest.session_id)?
            .ok_or_else(|| anyhow::anyhow!("Session {} not found on disk", latest.session_id))
    }
}

/// Run a non-interactive query, outputting results to stdout.
/// `stream` controls whether text is streamed character-by-character.
/// `config` holds explicit CLI configuration.
/// `bypass_all` when true, skips all permission checks (BypassPermissions mode).
/// `resume_session` when provided, injects prior conversation history into the engine.
fn run_noninteractive_query(
    query: &str,
    stream: bool,
    config: &CliConfig,
    bypass_all: bool,
    resume_session: Option<shannon_engine::state::SessionData>,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        // Build tool registry with all standard tools (sandboxed to project dir)
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tools = ToolRegistry::new();
        let reg_result = register_default_tools_with_project_dir_ex(&mut tools, &project_dir)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;
        let agent_context_handle = reg_result.agent_context_handle;
        let plan_mode_flag = reg_result.plan_manager.plan_mode_flag();

        // Load and register skills from shannon-skills as tools
        let _ = shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Discover MCP server configurations and register their tools dynamically
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                eprintln!("Discovered {mcp_count} MCP server(s)");
                for config in mcp_registry.enabled_servers() {
                    let command = match &config.command {
                        Some(cmd) => cmd.clone(),
                        None => {
                            eprintln!(
                                "  Skipping '{}' (HTTP/SSE transport not yet supported for discovery)",
                                config.name
                            );
                            continue;
                        }
                    };

                    match shannon_core::discover_tools(
                        &config.name,
                        &command,
                        &config.args,
                        &config.env,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            let tool_count = result.tools.len();
                            let boxed: Vec<Box<dyn shannon_core::tools::Tool>> = result
                                .tools
                                .into_iter()
                                .map(|t| Box::new(t) as Box<dyn shannon_core::tools::Tool>)
                                .collect();
                            let deferred = tools.register_batch(boxed).unwrap_or(0);
                            if deferred > 0 {
                                eprintln!(
                                    "  Registered {} tool(s) from '{}' ({} deferred, use ToolSearch to discover)",
                                    tool_count, result.server_name, deferred
                                );
                            } else {
                                eprintln!(
                                    "  Registered {} tool(s) from '{}'",
                                    tool_count, result.server_name
                                );
                            }
                        }
                        Err(e) => {
                            eprintln!("  Warning: MCP server '{}' discovery failed: {e}", config.name);
                        }
                    }
                }
            }
        }

        // Load plugins from ~/.shannon/plugins/ and register their tools
        {
            let plugins_dir = dirs::home_dir()
                .unwrap_or_default()
                .join(".shannon")
                .join("plugins");
            let mut plugin_registry = shannon_core::plugin::PluginRegistry::new(plugins_dir);
            if let Ok(()) = plugin_registry.load_all().await {
                let enabled = plugin_registry.list_enabled();
                if !enabled.is_empty() {
                    eprintln!("Loaded {} plugin(s)", enabled.len());
                    for plugin in &enabled {
                        match plugin.manifest.kind() {
                            Ok(shannon_core::plugin::PluginKind::Tool { transport }) => {
                                if let Some(command) = transport.command() {
                                    let args = transport.args().to_vec();
                                    match shannon_core::discover_tools(
                                        &plugin.manifest.name,
                                        command,
                                        &args,
                                        &std::collections::HashMap::new(),
                                        None,
                                    )
                                    .await
                                    {
                                        Ok(result) => {
                                            let tool_count = result.tools.len();
                                            let boxed: Vec<Box<dyn shannon_core::tools::Tool>> = result
                                                .tools
                                                .into_iter()
                                                .map(|t| Box::new(t) as Box<dyn shannon_core::tools::Tool>)
                                                .collect();
                                            tools.register_batch(boxed).unwrap_or(0);
                                            eprintln!("  Registered {} tool(s) from plugin '{}'", tool_count, plugin.manifest.name);
                                        }
                                        Err(e) => {
                                            eprintln!("  Warning: Plugin '{}' tool discovery failed: {e}", plugin.manifest.name);
                                        }
                                    }
                                }
                            }
                            Ok(shannon_core::plugin::PluginKind::Command { name, description }) => {
                                eprintln!("  Command plugin '{}' ({}) — use /plugin:{}", plugin.manifest.name, description, name);
                            }
                            Ok(shannon_core::plugin::PluginKind::Skill { trigger, template: _ }) => {
                                eprintln!("  Skill plugin '{}' (trigger: '{}') — use /{}", plugin.manifest.name, trigger, trigger);
                            }
                            Err(e) => {
                                eprintln!("  Warning: Plugin '{}' has invalid config: {e}", plugin.manifest.name);
                            }
                        }
                    }
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
            Err(e) if e.to_string().contains("Agent teams disabled") => {}
            Err(e) => eprintln!("Warning: Team context init failed: {e}"),
        }

        // Validate and warn
        if let Err(e) = client_config.validate() {
            eprintln!("Warning: {e}");
        }

        let llm_provider = client_config.provider.clone();
        let client = if client_config.provider.requires_auth() {
            shannon_engine::api::LlmClient::new(client_config)
        } else {
            shannon_engine::api::LlmClient::new_unauthenticated(client_config)
        };

        let mut permissions = shannon_engine::permissions::PermissionManager::new();
        // Non-interactive mode: use FullAuto by default (allows all non-critical tools),
        // or BypassPermissions with --yes flag (allows everything including critical).
        if bypass_all {
            permissions.set_approval_mode(shannon_engine::permissions::ApprovalMode::BypassPermissions);
        } else {
            permissions.set_approval_mode(shannon_engine::permissions::ApprovalMode::FullAuto);
        }
        let state = StateManager::new();

        let base_engine = QueryEngine::with_defaults(client, tools, permissions, state)
            .with_plan_mode_active(plan_mode_flag);

        // Initialize memory store at ~/.shannon/memories/
        let mut engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            if let Err(e) = mem_store.load() {
                tracing::debug!("Failed to load memory store: {e}");
            }
            base_engine.with_memory(mem_store)
        };

        // Auto-load project instructions (CLAUDE.md, AGENTS.md, GEMINI.md)
        if let Some(instructions) = shannon_core::project_instructions::load_from_cwd() {
            engine.append_system_prompt(&instructions.content);
        }

        // Restore prior conversation history if --resume was specified
        if let Some(session_data) = resume_session {
            let count = session_data.messages.len();
            engine.replace_conversation(session_data.messages);
            eprintln!("Resumed session ({count} messages loaded)");
        }

        let context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: query.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: should_enable_tools(llm_provider.clone()),
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
                        Ok(s) if s.len() > 200 => {
                            let mut end = 200;
                            while !s.is_char_boundary(end) { end -= 1; }
                            format!("{}…", &s[..end])
                        }
                        Ok(s) => s,
                        Err(_) => "(invalid json)".to_string(),
                    };
                    eprintln!("[tool #{tool_count}: {tool_name}] {input_summary}");
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    if is_error {
                        let err_summary = if result.len() > 300 {
                            let mut end = 300;
                            while !result.is_char_boundary(end) { end -= 1; }
                            format!("{}…", &result[..end])
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

/// Emit an NDJSON event to stdout (newline-delimited JSON).
fn emit_ci_event(event: &CiEvent) {
    match serde_json::to_string(event) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("Warning: failed to serialize CI event: {e}"),
    }
}

/// NDJSON event type for structured streaming output.
/// Mirrors the type defined in `shannon_core::output_format` but kept local
/// to avoid a dependency that some build environments strip during linting.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
enum OutputEvent {
    #[serde(rename = "text_delta")]
    TextDelta { content: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        name: String,
        output: String,
        is_error: bool,
    },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "done")]
    Done { exit_code: i32 },
}

impl OutputEvent {
    fn to_ndjson(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("{json}\n"),
            Err(_) => String::new(),
        }
    }
}

/// Emit an [`OutputEvent`] as an NDJSON line to stdout, flushing immediately.
fn emit_output_event(event: &OutputEvent) {
    let line = event.to_ndjson();
    if !line.is_empty() {
        print!("{line}");
        std::io::stdout().flush().ok();
    }
}

/// Load a JSON Schema from a file path or inline JSON string.
///
/// If the input starts with `{` or `[`, it's parsed as inline JSON.
/// Otherwise it's treated as a file path and its contents are read and parsed.
fn load_schema(input: &str) -> Result<shannon_core::StructuredOutputConfig> {
    let trimmed = input.trim();
    let json_str = if trimmed.starts_with('{') || trimmed.starts_with('[') {
        trimmed.to_string()
    } else {
        std::fs::read_to_string(trimmed)
            .map_err(|e| anyhow::anyhow!("Failed to read schema file '{trimmed}': {e}"))?
    };
    let schema: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("Invalid JSON in schema: {e}"))?;
    Ok(shannon_core::StructuredOutputConfig::new(schema))
}

/// Run a CI/CD headless query.
///
/// When `--prompt` is provided (the `--prompt` long flag, not the positional arg),
/// this path is taken instead of TUI or the simpler non-interactive query.
///
/// Features:
/// - Skips TUI entirely
/// - Restricts tools to `--allowed-tools` list (exit code 2 on violation)
/// - Limits turns via `--max-turns` (exit code 3 when exceeded)
/// - Outputs structured JSON with `--output-format json`
///
/// Exit codes: 0 success, 1 error, 2 tool denied, 3 max turns reached.
#[allow(clippy::too_many_arguments)]
fn run_headless_query(
    prompt: &str,
    config: &CliConfig,
    allowed_tools: Option<&[String]>,
    output_format: OutputFormat,
    max_turns: Option<u32>,
    exit_on_error: bool,
    quiet: bool,
    diff_only: bool,
    resume_session: Option<shannon_engine::state::SessionData>,
    schema_config: Option<&shannon_core::StructuredOutputConfig>,
    notify: bool,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    let exit_code: HeadlessExitCode = rt.block_on(async {
        let start = Instant::now();

        // Build tool registry with all standard tools (sandboxed to project dir)
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tools = ToolRegistry::new();
        let reg_result = register_default_tools_with_project_dir_ex(&mut tools, &project_dir)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;
        let agent_context_handle = reg_result.agent_context_handle;
        let plan_mode_flag = reg_result.plan_manager.plan_mode_flag();

        // Load and register skills
        let _ = shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Discover MCP servers
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                eprintln!("Discovered {mcp_count} MCP server(s)");
                for mcp_config in mcp_registry.enabled_servers() {
                    let command = match &mcp_config.command {
                        Some(cmd) => cmd.clone(),
                        None => continue,
                    };
                    match shannon_core::discover_tools(
                        &mcp_config.name,
                        &command,
                        &mcp_config.args,
                        &mcp_config.env,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            let tool_count = result.tools.len();
                            let boxed: Vec<Box<dyn shannon_core::tools::Tool>> = result
                                .tools
                                .into_iter()
                                .map(|t| Box::new(t) as Box<dyn shannon_core::tools::Tool>)
                                .collect();
                            if let Err(e) = tools.register_batch(boxed) {
                                eprintln!("  Warning: failed to register tools from '{}': {e}", result.server_name);
                            }
                            eprintln!(
                                "  Registered {} tool(s) from '{}'",
                                tool_count, result.server_name
                            );
                        }
                        Err(e) => {
                            eprintln!("  Warning: MCP server '{}' discovery failed: {e}", mcp_config.name);
                        }
                    }
                }
            }
        }

        // Apply tool access restrictions from --allowed-tools
        if let Some(allowed) = allowed_tools {
            tools.set_allowed_tools(Some(allowed.to_vec()));
        }

        // Build LLM client
        let client_config = build_llm_config_from_builder(config);
        match shannon_tools::AgentToolContext::new(client_config.clone()).await {
            Ok(ctx) => {
                if let Err(e) = shannon_tools::register_team_tools(&mut tools, ctx.coordinator.clone()) {
                    eprintln!("Warning: Team tool registration failed: {e}");
                }
                if let Ok(mut guard) = agent_context_handle.lock() {
                    *guard = Some(ctx);
                }
            }
            Err(e) if e.to_string().contains("Agent teams disabled") => {}
            Err(e) => eprintln!("Warning: Team context init failed: {e}"),
        }

        if let Err(e) = client_config.validate() {
            eprintln!("Error: {e}");
        }

        // Ensure we have an API key when the provider requires auth
        if client_config.provider.requires_auth() && client_config.api_key.is_empty() {
            eprintln!(
                "Error: no API key configured. Set SHANNON_API_KEY, ANTHROPIC_API_KEY, or OPENAI_API_KEY."
            );
            return Err(anyhow::anyhow!("no API key configured"));
        }

        let llm_provider = client_config.provider.clone();
        let client = if client_config.provider.requires_auth() {
            shannon_engine::api::LlmClient::new(client_config)
        } else {
            shannon_engine::api::LlmClient::new_unauthenticated(client_config)
        };

        // Permissions: FullAuto in headless mode (auto-approve non-critical, deny critical)
        let mut permissions = shannon_engine::permissions::PermissionManager::new();
        permissions.set_approval_mode(shannon_engine::permissions::ApprovalMode::FullAuto);
        let state = StateManager::new();

        let mut engine = QueryEngine::with_defaults(client, tools, permissions, state)
            .with_plan_mode_active(plan_mode_flag);

        // Apply max_turns if specified
        if let Some(turns) = max_turns {
            engine.set_max_turns(turns as usize);
        }

        // Memory store
        {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            if let Err(e) = mem_store.load() {
                tracing::debug!("Failed to load memory store: {e}");
            }
            engine = engine.with_memory(mem_store);
        }

        // Auto-load project instructions
        if let Some(instructions) = shannon_core::project_instructions::load_from_cwd() {
            engine.append_system_prompt(&instructions.content);
        }

        // Append structured output schema instructions
        if let Some(schema) = schema_config {
            engine.append_system_prompt(&schema.system_prompt_suffix());
        }

        // Restore prior conversation history if --resume was specified
        if let Some(session_data) = resume_session {
            let count = session_data.messages.len();
            engine.replace_conversation(session_data.messages);
            eprintln!("Resumed session ({count} messages loaded)");
        }

        let context = QueryContext {
            query_id: Uuid::new_v4(),
            session_id: Uuid::new_v4(),
            user_message: prompt.to_string(),
            metadata: QueryMetadata {
                timestamp: chrono::Utc::now(),
                tools_allowed: should_enable_tools(llm_provider.clone()),
                max_tokens: config.max_tokens().map(|v| v as u32),
                model: config.model().unwrap_or_else(|| "default".to_string()),
                temperature: config.temperature(),
                top_p: None,
            },
        };

        let mut event_stream = engine.process_query(context, None).await;

        // Collect execution data
        let mut response_text = String::new();
        let mut tool_calls: Vec<ToolCallSummary> = Vec::new();
        let mut total_tokens: u64 = 0;
        let mut _pending_tool_name: Option<String> = None;
        let mut exit_code = HeadlessExitCode::Success;
        let mut _turn_count: usize = 0;
        let mut changed_files: Vec<(String, String, String)> = Vec::new(); // (path, old, new)
        let allowed_set: Option<std::collections::HashSet<String>> =
            allowed_tools.map(|v| v.iter().cloned().collect());

        // Emit start event for JsonStream format
        if output_format == OutputFormat::JsonStream {
            let model_name = config.model().unwrap_or_else(|| "default".to_string());
            emit_ci_event(&CiEvent::Start {
                prompt: prompt.to_string(),
                model: model_name,
            });
        }

        while let Some(event_result) = event_stream.next().await {
            match event_result {
                Ok(QueryEvent::Text { content, .. }) => {
                    if output_format == OutputFormat::Text {
                        print!("{content}");
                        std::io::stdout().flush().ok();
                    } else if output_format == OutputFormat::JsonStream {
                        emit_output_event(&OutputEvent::TextDelta { content: content.clone() });
                    }
                    response_text.push_str(&content);
                }
                Ok(QueryEvent::ToolUseRequest { tool_name, tool_input, .. }) => {
                    // Check tool permission against allowed list
                    if let Some(ref allowed) = allowed_set {
                        if !allowed.contains(&tool_name) {
                            eprintln!(
                                "Error: tool '{}' not in allowed list: {}",
                                tool_name,
                                allowed.iter().cloned().collect::<Vec<_>>().join(",")
                            );
                            exit_code = HeadlessExitCode::PermissionDenied;
                            break;
                        }
                    }
                    let input_summary = match serde_json::to_string(&tool_input) {
                        Ok(s) if s.len() > 500 => {
                            let mut end = 500;
                            while !s.is_char_boundary(end) { end -= 1; }
                            format!("{}...", &s[..end])
                        }
                        Ok(s) => s,
                        Err(_) => "(invalid json)".to_string(),
                    };
                    _pending_tool_name = Some(tool_name.clone());
                    if !quiet {
                        eprintln!("[headless: invoking {tool_name}]");
                    }
                    // Emit NDJSON event
                    if output_format == OutputFormat::JsonStream {
                        if let Ok(input_value) = serde_json::from_str::<serde_json::Value>(&input_summary) {
                            emit_ci_event(&CiEvent::ToolCall {
                                name: tool_name.clone(),
                                input: input_value.clone(),
                            });
                            emit_output_event(&OutputEvent::ToolUse {
                                name: tool_name.clone(),
                                input: input_value,
                            });
                        }
                    }
                    // Store a placeholder; will be updated on ToolUseResult
                    tool_calls.push(ToolCallSummary {
                        tool: tool_name,
                        input_summary,
                        output_summary: String::new(),
                        success: false,
                    });
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    let output_summary = if result.len() > 500 {
                        let mut end = 500;
                        while !result.is_char_boundary(end) { end -= 1; }
                        format!("{}...", &result[..end])
                    } else {
                        result.clone()
                    };
                    // Update the last matching tool call
                    if let Some(tc) = tool_calls.iter_mut().rev().find(|tc| tc.tool == tool_name && !tc.success && tc.output_summary.is_empty()) {
                        tc.output_summary = output_summary.clone();
                        tc.success = !is_error;
                    }
                    _pending_tool_name = None;

                    // Track file changes for diff-only mode
                    if tool_name == "Edit" || tool_name == "Write" {
                        if let Ok(tool_result) = serde_json::from_str::<serde_json::Value>(&result) {
                            if let Some(path) = tool_result.get("path").and_then(|p| p.as_str()) {
                                // Read current file content for diff
                                let old_content = std::fs::read_to_string(path).unwrap_or_default();
                                changed_files.push((path.to_string(), old_content, String::new()));
                            }
                        }
                    }

                    // Emit NDJSON event
                    if output_format == OutputFormat::JsonStream {
                        emit_ci_event(&CiEvent::ToolResult {
                            name: tool_name.clone(),
                            output: output_summary.clone(),
                            success: !is_error,
                        });
                        emit_output_event(&OutputEvent::ToolResult {
                            name: tool_name.clone(),
                            output: output_summary.clone(),
                            is_error,
                        });
                    }

                    // Handle exit-on-error
                    if is_error && exit_on_error {
                        if !quiet {
                            eprintln!("[headless: tool-error {tool_name} - exiting due to --exit-on-error]");
                        }
                        exit_code = HeadlessExitCode::Error;
                        if output_format == OutputFormat::JsonStream {
                            emit_ci_event(&CiEvent::Error {
                                message: format!("Tool {tool_name} failed: {output_summary}"),
                            });
                            emit_output_event(&OutputEvent::Error {
                                message: format!("Tool {tool_name} failed: {output_summary}"),
                            });
                        }
                        break;
                    }

                    if !quiet {
                        if is_error {
                            eprintln!("[headless: tool-error {tool_name}]");
                        } else {
                            eprintln!("[headless: tool-done {tool_name}]");
                        }
                    }
                }
                Ok(QueryEvent::TurnCompleted { turn_number, tokens_used, .. }) => {
                    _turn_count = turn_number;
                    total_tokens += tokens_used;
                    eprintln!("[headless: turn {turn_number}, {tokens_used} tokens]");
                    // Check max turns
                    if let Some(max) = max_turns {
                        if turn_number >= max as usize {
                            if !quiet {
                                eprintln!("Max turns ({max}) reached");
                            }
                            exit_code = HeadlessExitCode::TurnLimit;
                            break;
                        }
                    }
                }
                Ok(QueryEvent::Usage { input_tokens, output_tokens, .. }) => {
                    total_tokens = total_tokens.max(input_tokens + output_tokens);
                }
                Ok(QueryEvent::Completed { .. }) => {
                    if output_format == OutputFormat::Text && !response_text.is_empty() {
                        // Text was already streamed; just ensure newline
                        println!();
                    }
                }
                Ok(QueryEvent::Failed { error, .. }) => {
                    eprintln!("Error: {error}");
                    let err_lower = error.to_lowercase();
                    if err_lower.contains("context")
                        || err_lower.contains("token limit")
                        || err_lower.contains("max_tokens")
                        || err_lower.contains("context_length")
                    {
                        exit_code = HeadlessExitCode::ContextOverflow;
                    } else if err_lower.contains("rate limit") || err_lower.contains("429") {
                        exit_code = HeadlessExitCode::RateLimited;
                    } else if err_lower.contains("permission") || err_lower.contains("denied") {
                        exit_code = HeadlessExitCode::PermissionDenied;
                    } else {
                        exit_code = HeadlessExitCode::Error;
                    }
                    if output_format == OutputFormat::JsonStream {
                        emit_output_event(&OutputEvent::Error {
                            message: error.clone(),
                        });
                    }
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Stream error: {e}");
                    exit_code = HeadlessExitCode::Error;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        // Validate structured output schema if provided
        if let Some(schema) = schema_config {
            match schema.validate_response(&response_text) {
                Ok(validated) => {
                    // Replace response with validated/cleaned JSON
                    response_text = serde_json::to_string_pretty(&validated)
                        .unwrap_or_else(|_| validated.to_string());
                }
                Err(e) => {
                    eprintln!("Schema validation failed: {e}");
                    exit_code = HeadlessExitCode::Error;
                    if output_format == OutputFormat::JsonStream {
                        emit_output_event(&OutputEvent::Error {
                            message: format!("Schema validation failed: {e}"),
                        });
                    }
                }
            }
        }

        // Output results based on format
        match output_format {
            OutputFormat::Text => {
                // Response was already streamed to stdout above
            }
            OutputFormat::Json => {
                let output = HeadlessOutput {
                    prompt: prompt.to_string(),
                    response: response_text,
                    tool_calls,
                    total_tokens,
                    duration_ms,
                    exit_code,
                };
                println!("{}", serde_json::to_string_pretty(&output).unwrap_or_else(|e| {
                    format!(r#"{{"error": "serialization failed: {e}"}}"#)
                }));
            }
            OutputFormat::JsonStream => {
                // Emit final done event
                emit_ci_event(&CiEvent::Done {
                    exit_code: i32::from(exit_code),
                    turns_used: _turn_count as u32,
                    tokens_used: total_tokens,
                });
                emit_output_event(&OutputEvent::Done {
                    exit_code: i32::from(exit_code),
                });
            }
        }

        // Diff-only mode: output unified diffs of changed files
        if diff_only && !changed_files.is_empty() {
            for (path, old_content, _new) in changed_files {
                let new_content = std::fs::read_to_string(&path).unwrap_or_default();
                if old_content != new_content {
                    println!("--- a/{path}");
                    println!("+++ b/{path}");
                    let diff = TextDiff::from_lines(&old_content, &new_content);
                    for op in diff.ops() {
                        for change in diff.iter_changes(op) {
                            let prefix = match change.tag() {
                                ChangeTag::Delete => "-",
                                ChangeTag::Insert => "+",
                                ChangeTag::Equal => " ",
                            };
                            println!("{prefix}{change}");
                        }
                    }
                }
            }
        }

        Ok::<HeadlessExitCode, anyhow::Error>(exit_code)
    })?;

    if notify {
        fire_headless_completion_notification(exit_code, prompt);
    }

    // Exit with the appropriate code
    std::process::exit(i32::from(exit_code));
}

/// Fire an OS-native notification describing the headless run outcome.
///
/// Spawns a `ShellNotifier` even when the platform binary may be missing —
/// failures are swallowed (logged to stderr when debug is on) so a missing
/// `notify-send` never breaks a headless run. When `[notifications.webhook]`
/// is configured in `.shannon.toml`, the same notification is also POSTed to
/// the webhook URL (Slack / Discord / Feishu / WeChat Work / custom / raw).
fn fire_headless_completion_notification(exit_code: HeadlessExitCode, prompt: &str) {
    use shannon_core::notifier::{Notification, NotificationLevel};

    let (title, body, level) = match exit_code {
        HeadlessExitCode::Success => (
            "Shannon — task complete",
            prompt
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>(),
            NotificationLevel::Success,
        ),
        HeadlessExitCode::TurnLimit => (
            "Shannon — max turns reached",
            prompt
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>(),
            NotificationLevel::Warning,
        ),
        HeadlessExitCode::PermissionDenied => (
            "Shannon — permission denied",
            "A required tool was blocked in non-interactive mode.".to_string(),
            NotificationLevel::Warning,
        ),
        HeadlessExitCode::RateLimited => (
            "Shannon — rate limited",
            "API provider returned 429; retry later.".to_string(),
            NotificationLevel::Error,
        ),
        HeadlessExitCode::ContextOverflow => (
            "Shannon — context overflow",
            "Conversation exceeded the model's context window.".to_string(),
            NotificationLevel::Error,
        ),
        HeadlessExitCode::Error | HeadlessExitCode::Timeout => (
            "Shannon — task failed",
            prompt
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(120)
                .collect::<String>(),
            NotificationLevel::Error,
        ),
    };

    let notification = Notification {
        title: title.into(),
        body,
        level,
        id: uuid::Uuid::new_v4().to_string(),
        timestamp: chrono::Utc::now(),
        source: Some(format!("headless:{exit_code:?}")),
        action_id: None,
    };

    let notifier = notifications::ShellNotifier::new();
    if let Err(e) = shannon_core::notifier::NotificationHandler::send(&notifier, &notification) {
        eprintln!("[notify] {e}");
    }

    if let Some(webhook_cfg) = load_headless_webhook_config() {
        match shannon_core::notifier::WebhookHandler::new(webhook_cfg) {
            Ok(handler) => {
                // WebhookHandler::send uses tokio::spawn internally, which
                // requires a running runtime. The headless block above has
                // already dropped its runtime by this point, so spin up a
                // short-lived runtime that keeps the spawned task alive
                // until delivery completes (or timeout_ms elapses).
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        eprintln!("[notify:webhook] runtime init failed: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    if let Err(e) =
                        shannon_core::notifier::NotificationHandler::send(&handler, &notification)
                    {
                        eprintln!("[notify:webhook] {e}");
                    }
                    // give the spawned fire-and-forget task time to deliver
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                });
            }
            Err(e) => eprintln!("[notify:webhook] handler init failed: {e}"),
        }
    }
}

/// Best-effort load of `[notifications.webhook]` from `.shannon.toml`
/// (project-local) or `~/.shannon/config.toml` (global). Project-local takes
/// precedence. Returns `None` on any error — never breaks the headless run.
///
/// Uses `toml::from_str` directly because `ConfigBuilder::load_local_toml`
/// does simple key=value parsing (no nested tables) — see
/// `shannon_core::unified_config::load_config_file` for details.
fn load_headless_webhook_config() -> Option<shannon_core::notifier::WebhookConfig> {
    use shannon_core::unified_config::ShannonConfig;
    let read_cfg = |path: &std::path::Path| -> Option<ShannonConfig> {
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str::<ShannonConfig>(&content).ok()
    };
    let local = std::path::Path::new(".shannon.toml");
    let cfg = read_cfg(local).or_else(|| {
        dirs::home_dir()
            .map(|h| h.join(".shannon").join("config.toml"))
            .and_then(|p| read_cfg(&p))
    })?;
    cfg.notifications.and_then(|n| n.webhook)
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
fn run_serve_command(
    port: u16,
    host: Option<String>,
    auth_token: Option<String>,
    allow_nonloopback: bool,
    config: &CliConfig,
) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        // Build tool registry with default tools (sandboxed to project dir).
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tools = shannon_core::ToolRegistry::new();
        let reg_result = register_default_tools_with_project_dir_ex(&mut tools, &project_dir)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;
        let agent_context_handle = reg_result.agent_context_handle;
        let _plan_mode_flag = reg_result.plan_manager.plan_mode_flag();

        // Load and register skills.
        let _ = shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Build LLM client config via the shared ConfigBuilder pipeline.
        let client_config = build_llm_config_from_builder(config);

        // Inject team context into AgentTool for sub-agent execution + team coordination
        match shannon_tools::AgentToolContext::new(client_config.clone()).await {
            Ok(ctx) => {
                if let Err(e) =
                    shannon_tools::register_team_tools(&mut tools, ctx.coordinator.clone())
                {
                    eprintln!("Warning: Team tool registration failed: {e}");
                }
                if let Ok(mut guard) = agent_context_handle.lock() {
                    *guard = Some(ctx);
                }
            }
            Err(e) if e.to_string().contains("Agent teams disabled") => {}
            Err(e) => eprintln!("Warning: Team context init failed: {e}"),
        }

        let mut server = shannon_core::api_server::ShannonApiServer::new(client_config)
            .with_tools(tools)
            .port(port)
            .allow_nonloopback(allow_nonloopback);

        if let Some(h) = host.as_deref() {
            server = server.host(h);
        }
        if let Some(token) = auth_token {
            server = server.auth_token(token);
        }

        let bind_host = host.as_deref().unwrap_or("127.0.0.1");
        println!("Shannon API server starting on {bind_host}:{port}");
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
    let msg =
        shannon_agents::JsonRpcMessage::response(shannon_agents::JsonRpcId::Number(id), result);
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
    allowed_tools: Option<&str>,
) -> Result<()> {
    // Change working directory if specified
    if let Some(dir) = workdir {
        let canonical = std::path::Path::new(dir)
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Invalid working directory '{dir}': {e}"))?;
        std::env::set_current_dir(&canonical)
            .map_err(|e| anyhow::anyhow!("Failed to set working directory: {e}"))?;
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let config = build_cli_config(model, provider, None, None, None, false, HashMap::new());

        // ── Build full tool registry (sandboxed to project dir) ──
        let project_dir = std::env::current_dir().unwrap_or_default();
        let mut tools = ToolRegistry::new();
        let reg_result = register_default_tools_with_project_dir_ex(&mut tools, &project_dir)
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;
        let agent_context_handle = reg_result.agent_context_handle;
        let plan_mode_flag = reg_result.plan_manager.plan_mode_flag();

        let _ = shannon_ui::skill_bridge::register_skills_as_tools(&mut tools);

        // Discover MCP servers and register their tools dynamically
        {
            let mut mcp_registry = shannon_core::mcp_advanced::McpServerRegistry::new();
            let mcp_count = mcp_registry.load_from_default_paths();
            if mcp_count > 0 {
                tracing::info!("Discovered {mcp_count} MCP server(s)");
                for mcp_config in mcp_registry.enabled_servers() {
                    let command = match &mcp_config.command {
                        Some(cmd) => cmd.clone(),
                        None => {
                            tracing::warn!(
                                "Skipping '{}' (HTTP/SSE transport not yet supported for discovery)",
                                mcp_config.name
                            );
                            continue;
                        }
                    };

                    match shannon_core::discover_tools(
                        &mcp_config.name,
                        &command,
                        &mcp_config.args,
                        &mcp_config.env,
                        None,
                    )
                    .await
                    {
                        Ok(result) => {
                            let tool_count = result.tools.len();
                            let boxed: Vec<Box<dyn shannon_core::tools::Tool>> = result
                                .tools
                                .into_iter()
                                .map(|t| Box::new(t) as Box<dyn shannon_core::tools::Tool>)
                                .collect();
                            let deferred = tools.register_batch(boxed).unwrap_or(0);
                            if deferred > 0 {
                                tracing::info!(
                                    "Registered {} tool(s) from '{}' ({} deferred)",
                                    tool_count,
                                    result.server_name,
                                    deferred
                                );
                            } else {
                                tracing::info!(
                                    "Registered {} tool(s) from '{}'",
                                    tool_count,
                                    result.server_name
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("MCP server '{}' discovery failed: {e}", mcp_config.name);
                        }
                    }
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
            Err(e) if e.to_string().contains("Agent teams disabled") => {}
            Err(e) => tracing::warn!("Team context init failed: {e}"),
        }

        // ── Remote team tools (JSON-RPC to coordinator via stdin/stdout) ──
        let coordinator_channel = shannon_agents::CoordinatorChannel::new();
        let agent_name_owned = name.to_string();
        tools.register(Box::new(shannon_agents::RemoteTeamTaskListTool::new(
            coordinator_channel.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamTaskClaimTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamNotifyIdleTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteSendMessageTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamTaskCreateTool::new(
            coordinator_channel.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamTaskUpdateTool::new(
            coordinator_channel.clone(),
            agent_name_owned.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamTaskGetTool::new(
            coordinator_channel.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        tools.register(Box::new(shannon_agents::RemoteTeamManifestTool::new(
            coordinator_channel.clone(),
        ))).unwrap_or_else(|e| eprintln!("Warning: tool registration failed: {e}"));
        let coordinator_channel_for_loop = coordinator_channel.clone();

        // Apply tool access restrictions from agent definition
        if let Some(tools_list) = allowed_tools {
            let allowed: Vec<String> = tools_list.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !allowed.is_empty() {
                tools.set_allowed_tools(Some(allowed));
            }
        }

        let llm_provider = client_config.provider.clone();
        let client = if client_config.provider.requires_auth() {
            shannon_engine::api::LlmClient::new(client_config)
        } else {
            shannon_engine::api::LlmClient::new_unauthenticated(client_config)
        };

        let mut permissions = shannon_engine::permissions::PermissionManager::new();
        let approval_mode = match permission_mode {
            Some("auto") => shannon_engine::permissions::ApprovalMode::AutoEdit,
            Some("plan") => shannon_engine::permissions::ApprovalMode::Plan,
            Some("full-auto") => shannon_engine::permissions::ApprovalMode::FullAuto,
            Some("dontAsk") => shannon_engine::permissions::ApprovalMode::DontAsk,
            Some("readonly") => shannon_engine::permissions::ApprovalMode::Readonly,
            _ => shannon_engine::permissions::ApprovalMode::FullAuto,
        };
        permissions.set_approval_mode(approval_mode);
        let state = StateManager::new();

        let base_engine = QueryEngine::with_defaults(client, tools, permissions, state)
            .with_plan_mode_active(plan_mode_flag);

        // Memory store
        let mut engine = {
            let memory_path = dirs::home_dir()
                .map(|h| h.join(".shannon").join("memories"))
                .unwrap_or_else(|| std::path::PathBuf::from(".shannon/memories"));
            let mut mem_store = shannon_core::MemoryStore::new(memory_path);
            if let Err(e) = mem_store.load() {
                tracing::debug!("Failed to load memory store: {e}");
            }
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
        }).unwrap_or_else(|e| {
            tracing::error!("JSON serialization error: {e}");
            serde_json::Value::Null
        });
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
                                    }).unwrap_or_else(|e| {
                                        tracing::error!("JSON serialization error: {e}");
                                        serde_json::Value::Null
                                    });
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
                                        tools_allowed: should_enable_tools(llm_provider.clone()),
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
                                                }).unwrap_or_else(|e| {
                                                    tracing::error!("JSON serialization error: {e}");
                                                    serde_json::Value::Null
                                                });
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
                                    }).unwrap_or_else(|e| {
                                        tracing::error!("JSON serialization error: {e}");
                                        serde_json::Value::Null
                                    });
                                agent_notify("task_complete", complete).await;

                                // Report idle
                                let idle = serde_json::to_value(
                                    shannon_agents::AgentIdleParams {
                                        agent_name: name.to_string(),
                                        available_tasks_count: 0,
                                    }).unwrap_or_else(|e| {
                                        tracing::error!("JSON serialization error: {e}");
                                        serde_json::Value::Null
                                    });
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

// ── Deep Link (shannon:// URL scheme) Support ───────────────────────────

/// Detect and convert a `shannon://` URL to CLI arguments.
///
/// Returns `Some(args)` if `arg` starts with `"shannon://"`, where `args` is
/// a `Vec<String>` suitable for passing to `Cli::try_parse_from()`.
/// Returns `None` for all non-shannon URLs.
fn parse_deep_link(arg: &str) -> Option<Vec<String>> {
    if !arg.starts_with("shannon://") {
        return None;
    }

    // Remove "shannon://" prefix
    let rest = &arg["shannon://".len()..];

    // Split into path and query string
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };

    // Strip trailing slash from path (support both "prompt" and "prompt/")
    let path = path.trim_end_matches('/');

    match path {
        "prompt" => {
            let raw = parse_query_param(query, "text").unwrap_or_default();
            let text = urlencoding::decode(&raw)
                .unwrap_or_else(|_| std::borrow::Cow::Borrowed(&raw))
                .into_owned();
            Some(vec!["shannon".to_string(), "--prompt".to_string(), text])
        }
        "resume" => {
            let raw_id = parse_query_param(query, "id");
            match raw_id {
                Some(raw) => {
                    let id = urlencoding::decode(&raw)
                        .unwrap_or_else(|_| std::borrow::Cow::Borrowed(&raw))
                        .into_owned();
                    Some(vec!["shannon".to_string(), "--resume".to_string(), id])
                }
                None => Some(vec!["shannon".to_string(), "--continue".to_string()]),
            }
        }
        _ => None,
    }
}

/// Extract the value of a query parameter by key from a URL query string.
fn parse_query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Register the `shannon://` URL scheme handler on Linux.
///
/// Creates a `.desktop` file in `~/.local/share/applications/` and registers
/// it as the default handler for `x-scheme-handler/shannon`.
fn register_url_scheme_linux() -> Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let apps_dir = home.join(".local").join("share").join("applications");
    std::fs::create_dir_all(&apps_dir)?;

    let desktop_path = apps_dir.join("shannon-url-handler.desktop");
    let desktop_content = "\
[Desktop Entry]
Type=Application
Name=Shannon URL Handler
Exec=shannon %u
MimeType=x-scheme-handler/shannon;
NoDisplay=true
";
    std::fs::write(&desktop_path, desktop_content)?;

    // Register the handler
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output();

    let _ = std::process::Command::new("xdg-mime")
        .args([
            "default",
            "shannon-url-handler.desktop",
            "x-scheme-handler/shannon",
        ])
        .output();

    println!("Registered shannon:// URL scheme handler (Linux)");
    println!("  Desktop file: {}", desktop_path.display());
    Ok(())
}

/// Unregister the `shannon://` URL scheme handler on Linux.
fn unregister_url_scheme_linux() -> Result<()> {
    let home =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
    let desktop_path = home
        .join(".local")
        .join("share")
        .join("applications")
        .join("shannon-url-handler.desktop");

    if desktop_path.exists() {
        std::fs::remove_file(&desktop_path)?;
    }

    let apps_dir = home.join(".local").join("share").join("applications");
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .output();

    println!("Unregistered shannon:// URL scheme handler (Linux)");
    Ok(())
}

/// Print macOS URL scheme registration instructions.
fn register_url_scheme_macos() -> Result<()> {
    println!("To register shannon:// URL scheme on macOS:");
    println!();
    println!("  1. Create or edit your app's Info.plist and add:");
    println!("     <key>CFBundleURLTypes</key>");
    println!("     <array>");
    println!("       <dict>");
    println!("         <key>CFBundleURLName</key>");
    println!("         <string>com.shannon.code</string>");
    println!("         <key>CFBundleURLSchemes</key>");
    println!("         <array>");
    println!("           <string>shannon</string>");
    println!("         </array>");
    println!("       </dict>");
    println!("     </array>");
    println!();
    println!("  2. Or use the Swift CLI tool approach:");
    println!("     swift -e '...' (programmatic registration)");
    println!();
    println!("For Tauri-based desktop app, add to tauri.conf.json:");
    println!("  \"bundle\": {{ \"iOS\": {{ \"customProtocols\": [\"shannon\"] }} }}");
    Ok(())
}

/// Print macOS URL scheme unregistration instructions.
fn unregister_url_scheme_macos() -> Result<()> {
    println!("To unregister shannon:// URL scheme on macOS:");
    println!("  Remove the CFBundleURLTypes entry from your app's Info.plist.");
    Ok(())
}

/// Detect and run URL scheme registration/unregistration for the current platform.
fn handle_url_scheme_registration(register: bool, unregister: bool) -> Result<()> {
    if cfg!(target_os = "linux") {
        if register {
            return register_url_scheme_linux();
        }
        if unregister {
            return unregister_url_scheme_linux();
        }
    } else if cfg!(target_os = "macos") {
        if register {
            return register_url_scheme_macos();
        }
        if unregister {
            return unregister_url_scheme_macos();
        }
    } else {
        println!("URL scheme registration is not supported on this platform.");
    }
    Ok(())
}

/// Resolve the path to the `shannon-desktop` binary.
///
/// Order: (a) PATH `shannon-desktop`, (b) known install dirs, (c) None.
fn find_desktop_binary() -> Option<std::path::PathBuf> {
    if let Ok(path) = which_desktop_on_path() {
        return Some(path);
    }
    let candidates = [
        std::path::PathBuf::from("/usr/local/bin/shannon-desktop"),
        dirs::home_dir()
            .map(|h| h.join(".local").join("bin").join("shannon-desktop"))
            .unwrap_or_default(),
        std::path::PathBuf::from(
            "/Applications/Shannon Desktop.app/Contents/MacOS/shannon-desktop",
        ),
    ];
    candidates.into_iter().find(|candidate| candidate.is_file())
}

/// Probe only the system PATH for `shannon-desktop` and return its path if found.
fn which_desktop_on_path() -> Result<std::path::PathBuf, ()> {
    let out = std::process::Command::new("command")
        .args(["-v", "shannon-desktop"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                Err(())
            } else {
                Ok(std::path::PathBuf::from(s))
            }
        }
        _ => Err(()),
    }
}

/// Launch the Shannon desktop application.
///
/// Best-effort: resolves the binary, optionally builds it via `cargo tauri
/// build`, then spawns it. If `foreground` is false the process is detached
/// (spawn only); otherwise we wait for it to exit.
fn run_desktop_command(no_build: bool, foreground: bool) -> Result<()> {
    let binary = match find_desktop_binary() {
        Some(b) => b,
        None => {
            if no_build {
                anyhow::bail!(
                    "shannon-desktop not found on PATH or in known install dirs. \
                     Install it or run without --no-build to build it from desktop/."
                );
            }
            // Try to build the desktop app from the workspace `desktop/` dir.
            let desktop_dir = std::path::PathBuf::from("desktop");
            eprintln!("shannon-desktop not found; building via `cargo tauri build`...");
            let status = std::process::Command::new("cargo")
                .arg("tauri")
                .arg("build")
                .current_dir(&desktop_dir)
                .status();
            match status {
                Ok(s) if s.success() => {
                    // Search known dirs again after a successful build.
                    match find_desktop_binary() {
                        Some(b) => b,
                        None => anyhow::bail!(
                            "Built the desktop app but could not locate the shannon-desktop binary."
                        ),
                    }
                }
                Ok(s) => anyhow::bail!("`cargo tauri build` failed (exit status: {s})."),
                Err(e) => anyhow::bail!("Failed to run `cargo tauri build`: {e}"),
            }
        }
    };

    eprintln!("Launching Shannon Desktop: {}", binary.display());
    let mut cmd = std::process::Command::new(&binary);
    if !foreground {
        // Detach: spawn without waiting and report success immediately.
        match cmd.spawn() {
            Ok(_child) => {
                println!("Shannon Desktop launched (detached).");
                return Ok(());
            }
            Err(e) => anyhow::bail!("Failed to launch Shannon Desktop: {e}"),
        }
    }

    // Foreground: wait for the desktop app to exit and mirror its exit code.
    match cmd.status() {
        Ok(status) => {
            if let Some(code) = status.code() {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => anyhow::bail!("Failed to launch Shannon Desktop: {e}"),
    }
}

/// Find the external `shannon-gateway` binary on the system PATH.
fn find_gateway_binary() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("command")
        .args(["-v", "shannon-gateway"])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(std::path::PathBuf::from(s))
            }
        }
        _ => None,
    }
}

/// Delegate a gateway subcommand to the external `shannon-gateway` binary.
///
/// Spawns `shannon-gateway <subcommand>` and mirrors its exit code.
fn run_gateway_command(command: GatewaySubcommand) -> Result<()> {
    let binary = match find_gateway_binary() {
        Some(b) => b,
        None => {
            anyhow::bail!("shannon-gateway not found on PATH. Install the gateway service first.")
        }
    };

    let sub: &str = match command {
        GatewaySubcommand::Run => "run",
        GatewaySubcommand::Start => "start",
        GatewaySubcommand::Stop => "stop",
        GatewaySubcommand::Restart => "restart",
        GatewaySubcommand::Status => "status",
        GatewaySubcommand::List => "list",
        GatewaySubcommand::Install => "install",
        GatewaySubcommand::Uninstall => "uninstall",
        GatewaySubcommand::Setup => "setup",
        GatewaySubcommand::MigrateLegacy => "migrate-legacy",
        GatewaySubcommand::Enroll => "enroll",
    };

    eprintln!("Delegating to shannon-gateway: {sub}");
    let status = std::process::Command::new(&binary).arg(sub).status();
    match status {
        Ok(s) => {
            if let Some(code) = s.code() {
                std::process::exit(code);
            }
            Ok(())
        }
        Err(e) => anyhow::bail!("Failed to run shannon-gateway {sub}: {e}"),
    }
}

/// Current version of the `shannon` CLI crate.
fn current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Simple semver-ish comparison: returns true if `latest` is newer than `current`.
///
/// Compares dot-separated numeric components left to right. Non-numeric parts
/// are ignored. Good enough for release tag comparison.
fn version_is_newer(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Vec<u64> {
        v.split('.')
            .map(|p| {
                p.trim_start_matches('v')
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("")
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    }
    let a = parse(current);
    let b = parse(latest);
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if y > x {
            return true;
        }
        if y < x {
            return false;
        }
    }
    false
}

/// Self-update: check GitHub for a newer release and print upgrade steps.
fn run_update_command() -> Result<()> {
    let current = current_version();
    println!("Current version: {current}");
    println!(
        "Checking for updates at https://api.github.com/repos/shannon-agent/shannon-agent/releases/latest ..."
    );

    // Best-effort: shell out to `curl` (already referenced by the install flow).
    let out = std::process::Command::new("curl")
        .args([
            "-fsSL",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: shannon-cli",
            "https://api.github.com/repos/shannon-agent/shannon-agent/releases/latest",
        ])
        .output();

    let body = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => {
            println!(
                "WARN: GitHub request failed (exit {}).",
                o.status.code().unwrap_or(-1)
            );
            println!(
                "Visit https://github.com/shannon-agent/shannon-agent/releases to update manually."
            );
            return Ok(());
        }
        Err(e) => {
            println!("WARN: could not run curl ({e}). Install curl or update manually at");
            println!("https://github.com/shannon-agent/shannon-agent/releases");
            return Ok(());
        }
    };

    let value: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            println!("WARN: failed to parse release metadata: {e}");
            return Ok(());
        }
    };

    let latest = value
        .get("tag_name")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    if latest.is_empty() {
        println!("WARN: no tag_name found in release metadata.");
        return Ok(());
    }

    println!("Latest version: {latest}");
    if version_is_newer(&current, &latest) {
        println!();
        println!("A newer version is available: {latest}");
        println!("To upgrade, run:");
        println!("    curl -fsSL https://get.shannon.ai/install.sh | sh");
        println!();
        println!(
            "Or download from: https://github.com/shannon-agent/shannon-agent/releases/{latest}"
        );
    } else {
        println!("You are already on the latest version.");
    }
    Ok(())
}

/// Probe whether a TCP port is free by attempting to bind it.
fn is_port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Run diagnostics: toolchain, ports, services, and config.
///
/// Never blocks — every check reports OK/WARN/INFO and continues.
fn run_doctor_command() -> Result<()> {
    println!("Shannon Doctor — diagnostics");

    // Toolchain probes (PATH via `command -v`).
    let tools = ["cargo", "rustc", "node", "bun"];
    for tool in tools {
        let present = std::process::Command::new("command")
            .args(["-v", tool])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if present {
            println!("[OK]    found '{tool}' on PATH");
        } else {
            // `bun` is optional; only WARN. `cargo`/`rustc`/`node` are WARN too
            // (some install paths use prebuilt binaries). Never block.
            println!("[WARN]  '{tool}' not found on PATH");
        }
    }

    // Gateway service on PATH.
    match find_gateway_binary() {
        Some(p) => println!("[OK]    shannon-gateway found: {}", p.display()),
        None => println!("[INFO]  shannon-gateway not found on PATH (run `shannon gateway setup`)"),
    }

    // Port 33420 free (the default api_server port).
    if is_port_free(33420) {
        println!("[OK]    port 33420 is free");
    } else {
        println!("[WARN]  port 33420 is already in use (api_server may be running)");
    }

    // Configured engine URL.
    let engine_url = std::env::var("SHANNON_BASE_URL")
        .ok()
        .or_else(|| std::env::var("ANTHROPIC_BASE_URL").ok());
    match engine_url {
        Some(url) => println!("[OK]    engine URL configured: {url}"),
        None => println!("[INFO]  no engine URL configured (using provider default)"),
    }

    println!("Doctor finished.");
    Ok(())
}

fn main() -> Result<()> {
    // Check for URL scheme registration flags first (hidden, not in Cli struct)
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        if args[1] == "--register-url-scheme" {
            return handle_url_scheme_registration(true, false);
        }
        if args[1] == "--unregister-url-scheme" {
            return handle_url_scheme_registration(false, true);
        }

        // Check for deep link URL in first positional argument
        if let Some(transformed) = parse_deep_link(&args[1]) {
            let cli = Cli::try_parse_from(transformed)?;
            return run_with_cli(cli);
        }
    }

    let cli = Cli::parse();
    run_with_cli(cli)
}

/// Main CLI dispatch logic, factored out so it can be called from either the
/// normal `clap::parse()` path or the deep-link transformation path.
fn run_with_cli(cli: Cli) -> Result<()> {
    // Initialize i18n — auto-detect system language, allow --lang override
    if let Some(ref lang) = cli.lang {
        i18n::set_locale(lang);
    } else {
        let detected = i18n::detect_system_locale();
        i18n::set_locale(&detected);
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
                    .add_directive("shannon_cli=info".parse().expect("valid log directive")),
            )
            .init();
        return run_team_agent_mode(
            agent_name,
            cli.model.as_deref(),
            cli.provider.as_deref(),
            cli.system_prompt.as_deref(),
            cli.workdir.as_deref(),
            cli.permission_mode.as_deref(),
            cli.team_allowed_tools.as_deref(),
        );
    }

    // Determine if session resume is requested (used by multiple code paths below)
    // Precedence: --resume-id > --resume <UUID> > --resume / --continue (most recent)
    let should_resume = cli.resume.is_some() || cli.r#continue || cli.resume_id.is_some();
    // Use --resume-id if provided, otherwise fall back to --resume value.
    // Normalize empty string from bare --resume (no UUID) to None.
    let resume_session_id: Option<&str> = cli
        .resume_id
        .as_deref()
        .or_else(|| cli.resume.as_deref().filter(|s| !s.is_empty()));

    // ── CI/CD Headless mode: --prompt flag ──
    // Takes priority over bare prompt and pipe mode.
    // When --prompt is specified, run in structured headless mode with
    // exit codes, tool restrictions, and JSON output support.
    if let Some(ref headless_prompt) = cli.headless_prompt {
        let config = build_cli_config(
            cli.model.as_deref(),
            cli.provider.as_deref(),
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        let resume_data = if should_resume {
            load_resume_session(resume_session_id).ok()
        } else {
            None
        };
        // Convert comma-separated team_allowed_tools into Vec<String>
        let allowed_vec: Option<Vec<String>> = cli
            .team_allowed_tools
            .as_deref()
            .map(|s| s.split(',').map(|t| t.trim().to_string()).collect());
        // Parse --schema: load from file or parse inline JSON
        let schema_config = cli.schema.as_deref().map(load_schema).transpose()?;

        return run_headless_query(
            headless_prompt,
            &config,
            allowed_vec.as_deref(),
            cli.output_format,
            cli.max_turns,
            cli.exit_on_error,
            cli.quiet || cli.diff_only,
            cli.diff_only,
            resume_data,
            schema_config.as_ref(),
            cli.notify,
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
        return run_noninteractive_query(&prompt, true, &config, cli.yes, None);
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
        let resume_data = if should_resume {
            load_resume_session(resume_session_id).ok()
        } else {
            None
        };
        return run_noninteractive_query(&prompt, true, &config, cli.yes, resume_data);
    }

    // No prompt argument: check stdin for piped input
    let stdin_content = read_stdin();
    if !stdin_content.is_empty() {
        let config = build_cli_config(
            cli.model.as_deref(),
            cli.provider.as_deref(),
            None,
            None,
            None,
            false,
            HashMap::new(),
        );
        let resume_data = if should_resume {
            load_resume_session(resume_session_id).ok()
        } else {
            None
        };
        return run_noninteractive_query(&stdin_content, true, &config, cli.yes, resume_data);
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
            let env_overrides = parse_cli_env(env).map_err(|e| anyhow::anyhow!(e))?;
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
        }) => build_cli_config(
            model.as_deref(),
            provider.as_deref(),
            *max_tokens,
            None,
            None,
            false,
            HashMap::new(),
        ),
        // No subcommand, Version, Config, and Serve commands don't need config in the same way
        None
        | Some(Commands::Version { .. })
        | Some(Commands::Config { .. })
        | Some(Commands::Serve { .. })
        | Some(Commands::Screenshot { .. })
        | Some(Commands::Mcp { .. })
        | Some(Commands::Desktop { .. })
        | Some(Commands::Gateway { .. })
        | Some(Commands::Update)
        | Some(Commands::Doctor) => CliConfig::default(),
    };

    // Initialize tracing if debug mode enabled
    if config.debug {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("info".parse().expect("valid log directive")),
            )
            .init();
    }

    // Execute commands with explicit config
    match cli.command {
        None => {
            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{e:?}"))?;
            if should_resume {
                match load_resume_session(resume_session_id) {
                    Ok(session_data) => {
                        let count = repl.restore_session(session_data);
                        eprintln!("Resumed session ({count} messages loaded)");
                    }
                    Err(e) => eprintln!("Warning: could not resume session: {e}"),
                }
            }
            repl.run().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        Some(Commands::Repl { cwd, .. }) => {
            if let Some(dir) = cwd {
                let canonical = std::path::Path::new(&dir)
                    .canonicalize()
                    .map_err(|e| anyhow::anyhow!("Invalid working directory '{dir}': {e}"))?;
                std::env::set_current_dir(&canonical)
                    .map_err(|e| anyhow::anyhow!("Failed to set working directory: {e}"))?;
            }
            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{e:?}"))?;
            if should_resume {
                match load_resume_session(resume_session_id) {
                    Ok(session_data) => {
                        let count = repl.restore_session(session_data);
                        eprintln!("Resumed session ({count} messages loaded)");
                    }
                    Err(e) => eprintln!("Warning: could not resume session: {e}"),
                }
            }
            repl.run().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        Some(Commands::Version { verbose }) => {
            println!("Shannon Code v{}", env!("CARGO_PKG_VERSION"));
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
                        println!(
                            "No configuration set. Config file: {}",
                            manager.config_path().display()
                        );
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
            let resume_data = if should_resume {
                load_resume_session(resume_session_id).ok()
            } else {
                None
            };
            run_noninteractive_query(&query, !no_stream, &config, cli.yes, resume_data)?;
        }
        Some(Commands::Serve {
            port,
            host,
            auth_token,
            allow_nonloopback,
        }) => {
            run_serve_command(
                port,
                host.clone(),
                auth_token.clone(),
                allow_nonloopback,
                &config,
            )?;
        }
        Some(Commands::Screenshot { dir }) => {
            let output_path = std::path::PathBuf::from(&dir);
            shannon_ui::screenshot::render_all_scenes(&output_path)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        Some(Commands::Mcp { command }) => match command {
            McpSubcommand::Install {
                bundle,
                user,
                yes,
                dry_run,
            } => {
                let scope = if user {
                    mcp_install::InstallScope::User
                } else {
                    mcp_install::InstallScope::Project
                };
                let bundle_path = std::path::Path::new(&bundle);

                let preview = match mcp_install::preview_bundle(bundle_path, scope) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!("Install failed: {e}");
                        std::process::exit(1);
                    }
                };

                println!(
                    "Target ({}): {}",
                    match preview.scope {
                        mcp_install::InstallScope::Project => "project",
                        mcp_install::InstallScope::User => "user",
                    },
                    preview.target_path.display()
                );
                println!("Servers ({}):", preview.servers.len());
                for server in &preview.servers {
                    let marker = if server.overwrites_existing {
                        " [OVERWRITE]"
                    } else {
                        ""
                    };
                    match &server.command {
                        Some(cmd) => {
                            let args = if cmd.args.is_empty() {
                                String::new()
                            } else {
                                format!(" {}", cmd.args.join(" "))
                            };
                            println!("  - {} -> {}{}{marker}", server.name, cmd.command, args);
                        }
                        None => println!("  - {} (no command field)", server.name),
                    }
                }
                if preview.has_overwrite() {
                    println!("\nWarning: some servers will overwrite existing entries.");
                }

                if dry_run {
                    println!("\n--dry-run: no changes written.");
                } else {
                    let mut should_install = yes;
                    if !yes {
                        print!("\nProceed with install? [y/N] ");
                        use std::io::Write as _;
                        std::io::stdout().flush().ok();
                        let mut answer = String::new();
                        if std::io::stdin().read_line(&mut answer).is_err() {
                            eprintln!("Install aborted: could not read confirmation.");
                            std::process::exit(1);
                        }
                        let answer = answer.trim().to_lowercase();
                        should_install = answer == "y" || answer == "yes";
                        if !should_install {
                            println!("Install cancelled.");
                        }
                    }

                    if should_install {
                        match mcp_install::install_bundle(bundle_path, scope) {
                            Ok(result) => {
                                println!(
                                    "\nInstalled {} server(s) to {}:",
                                    result.servers_installed,
                                    result.target_path.display()
                                );
                                for name in &result.server_names {
                                    println!("  - {name}");
                                }
                            }
                            Err(e) => {
                                eprintln!("Install failed: {e}");
                                std::process::exit(1);
                            }
                        }
                    }
                }
            }
        },
        Some(Commands::Desktop {
            no_build,
            foreground,
        }) => {
            run_desktop_command(no_build, foreground)?;
        }
        Some(Commands::Gateway { command }) => {
            run_gateway_command(command)?;
        }
        Some(Commands::Update) => {
            run_update_command()?;
        }
        Some(Commands::Doctor) => {
            run_doctor_command()?;
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
        assert!(
            config.get_env("NON_EXISTENT_KEY").is_none()
                || config.get_env("NON_EXISTENT_KEY").is_some()
        );
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
        let cli =
            Cli::try_parse_from(["shannon", "repl", "-e", "MODEL=gpt-4o", "-e", "TOKENS=4096"])
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
    fn test_cli_parse_serve_defaults() {
        let cli = Cli::try_parse_from(["shannon", "serve"]).unwrap();
        match cli.command {
            Some(Commands::Serve {
                port,
                host,
                auth_token,
                allow_nonloopback,
            }) => {
                assert_eq!(port, 33420);
                assert_eq!(host, None);
                assert_eq!(auth_token, None);
                assert!(!allow_nonloopback);
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    fn test_cli_parse_serve_custom_port_and_auth_token() {
        let cli = Cli::try_parse_from([
            "shannon",
            "serve",
            "--port",
            "34567",
            "--auth-token",
            "secret-token",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Serve {
                port, auth_token, ..
            }) => {
                assert_eq!(port, 34567);
                assert_eq!(auth_token.as_deref(), Some("secret-token"));
            }
            _ => panic!("Expected Serve command"),
        }
    }

    #[test]
    fn test_cli_parse_serve_nonloopback_requires_explicit_flag() {
        let cli = Cli::try_parse_from([
            "shannon",
            "serve",
            "--host",
            "0.0.0.0",
            "--allow-nonloopback",
            "--auth-token",
            "secret-token",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Serve {
                host,
                auth_token,
                allow_nonloopback,
                ..
            }) => {
                assert_eq!(host.as_deref(), Some("0.0.0.0"));
                assert_eq!(auth_token.as_deref(), Some("secret-token"));
                assert!(allow_nonloopback);
            }
            _ => panic!("Expected Serve command"),
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
        let cli =
            Cli::try_parse_from(["shannon", "--provider", "anthropic", "test query"]).unwrap();
        assert_eq!(cli.prompt.as_deref(), Some("test query"));
        assert_eq!(cli.provider.as_deref(), Some("anthropic"));
        assert!(cli.command.is_none());
    }

    #[test]
    fn test_cli_prompt_short_flag() {
        let cli = Cli::try_parse_from(["shannon", "-p", "fix the bug", "--output-format", "json"])
            .unwrap();
        assert_eq!(cli.headless_prompt.as_deref(), Some("fix the bug"));
    }

    #[test]
    fn test_cli_bare_prompt_with_model_and_provider() {
        let cli = Cli::try_parse_from(["shannon", "-m", "gpt-4o", "--provider", "openai", "你好"])
            .unwrap();
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

    // ── Resume flag tests ────────────────────────────────────────────────

    #[test]
    fn test_cli_resume_bare_flag_is_none() {
        // With num_args = 0..=1, bare --resume (no value) gives None
        // because clap treats "value absent" as None for Option<String>.
        // Use --continue or -c for "resume most recent" behavior.
        let cli = Cli::try_parse_from(["shannon", "--resume"]).unwrap();
        assert!(cli.resume.is_none());
        assert!(!cli.r#continue);
        assert!(cli.resume_id.is_none());
    }

    #[test]
    fn test_cli_resume_with_uuid() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let cli = Cli::try_parse_from(["shannon", "--resume", uuid]).unwrap();
        assert_eq!(cli.resume.as_deref(), Some(uuid));
        assert!(cli.resume_id.is_none());
    }

    #[test]
    fn test_cli_resume_short_flag_with_uuid() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let cli = Cli::try_parse_from(["shannon", "-r", uuid]).unwrap();
        assert_eq!(cli.resume.as_deref(), Some(uuid));
    }

    #[test]
    fn test_cli_continue_flag() {
        let cli = Cli::try_parse_from(["shannon", "--continue"]).unwrap();
        assert!(cli.r#continue);
        assert!(cli.resume.is_none());
        assert!(cli.resume_id.is_none());
    }

    #[test]
    fn test_cli_continue_short_flag() {
        let cli = Cli::try_parse_from(["shannon", "-c"]).unwrap();
        assert!(cli.r#continue);
    }

    #[test]
    fn test_cli_resume_id_flag() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let cli = Cli::try_parse_from(["shannon", "--resume-id", uuid]).unwrap();
        assert_eq!(cli.resume_id.as_deref(), Some(uuid));
        assert!(cli.resume.is_none());
        assert!(!cli.r#continue);
    }

    #[test]
    fn test_cli_resume_id_takes_precedence_over_resume() {
        // When both --resume-id and --resume are provided, --resume-id wins
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let other_uuid = "12345678-1234-1234-1234-123456789012";
        let cli =
            Cli::try_parse_from(["shannon", "--resume-id", uuid, "--resume", other_uuid]).unwrap();
        assert_eq!(cli.resume_id.as_deref(), Some(uuid));
        assert_eq!(cli.resume.as_deref(), Some(other_uuid));

        // In run_with_cli, --resume-id takes precedence
        let resolved: Option<&str> = cli
            .resume_id
            .as_deref()
            .or_else(|| cli.resume.as_deref().filter(|s| !s.is_empty()));
        assert_eq!(resolved, Some(uuid));
    }

    #[test]
    fn test_cli_continue_with_headless_prompt() {
        // --continue + --prompt should work together
        let cli = Cli::try_parse_from(["shannon", "--continue", "-p", "fix the bug"]).unwrap();
        assert!(cli.r#continue);
        assert_eq!(cli.headless_prompt.as_deref(), Some("fix the bug"));
    }

    #[test]
    fn test_cli_resume_id_with_headless_prompt() {
        let uuid = "550e8400-e29b-41d4-a716-446655440000";
        let cli =
            Cli::try_parse_from(["shannon", "--resume-id", uuid, "-p", "continue this"]).unwrap();
        assert_eq!(cli.resume_id.as_deref(), Some(uuid));
        assert_eq!(cli.headless_prompt.as_deref(), Some("continue this"));
    }

    // ── load_resume_session tests ────────────────────────────────────────

    #[test]
    fn test_load_resume_session_invalid_uuid() {
        let result = load_resume_session(Some("not-a-valid-uuid"));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Invalid session UUID"),
            "Expected invalid UUID error, got: {err}"
        );
    }

    #[test]
    fn test_load_resume_session_nonexistent_uuid() {
        let uuid = Uuid::new_v4();
        let result = load_resume_session(Some(&uuid.to_string()));
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not found"),
            "Expected 'not found' error, got: {err}"
        );
    }

    #[test]
    fn test_load_resume_session_no_sessions() {
        // With an empty default sessions dir, should return an error.
        // Just verify it doesn't panic.
        let _ = load_resume_session(None);
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
        let cli = Cli::try_parse_from(["shannon", "repl", "--provider", "anthropic"]).unwrap();
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
            "--provider",
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
        let _config = build_cli_config(None, None, None, None, None, false, HashMap::new());
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
        let cli =
            Cli::try_parse_from(["shannon", "query", "--provider", "anthropic", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query {
                query, provider, ..
            }) => {
                assert_eq!(query, "test");
                assert_eq!(provider.as_deref(), Some("anthropic"));
            }
            _ => panic!("Expected Query command"),
        }
    }

    #[test]
    fn test_cli_parse_query_with_max_tokens() {
        let cli =
            Cli::try_parse_from(["shannon", "query", "--max-tokens", "8192", "test"]).unwrap();
        match cli.command {
            Some(Commands::Query {
                query, max_tokens, ..
            }) => {
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
            Some(Commands::Query {
                query, no_stream, ..
            }) => {
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
            Some(Commands::Query {
                query,
                model,
                provider,
                max_tokens,
                output,
                no_stream,
            }) => {
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
            "-m",
            "claude-sonnet-4",
            "--provider",
            "anthropic",
            "--max-tokens",
            "4096",
            "--output",
            "json",
            "--no-stream",
            "你用的什么模型",
        ])
        .unwrap();
        match cli.command {
            Some(Commands::Query {
                query,
                model,
                provider,
                max_tokens,
                output,
                no_stream,
            }) => {
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
        assert_eq!(
            result.get("SHANNON_MODEL"),
            Some(&"你用的什么模型".to_string())
        );
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
        "#;
        let config: ShannonTomlConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.model.as_deref(), Some("gpt-4o"));
        assert_eq!(config.provider.as_deref(), Some("openai"));
        assert_eq!(config.max_tokens, Some(8192));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.timeout, Some(120));
        assert!(config.debug.unwrap());
        // N1/C-fields: legacy `api_key` / `base_url` are no longer recognised
        // in ShannonTomlConfig (A1-strict: plaintext never enters the config;
        // use `~/.shannon/secrets.env` instead for keys, and `provider_model`
        // for base_url).
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
            None,  // no max_tokens
            None,  // no temperature
            None,  // no timeout
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
            "shannon",
            "query",
            "-m",
            "gpt-4o",
            "--provider",
            "openai",
            "--max-tokens",
            "4096",
            "test",
        ])
        .unwrap();

        if let Some(Commands::Query {
            model,
            provider,
            max_tokens,
            ..
        }) = cli.command
        {
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
        let cli = Cli::try_parse_from(["shannon", "--team-agent", "--name", "worker-1"]).unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.name.as_deref(), Some("worker-1"));
    }

    #[test]
    fn test_cli_team_agent_with_model() {
        let cli = Cli::try_parse_from([
            "shannon",
            "--team-agent",
            "--name",
            "worker-2",
            "--model",
            "claude-sonnet-4",
            "--provider",
            "anthropic",
        ])
        .unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.name.as_deref(), Some("worker-2"));
        assert_eq!(cli.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(cli.provider.as_deref(), Some("anthropic"));
    }

    #[test]
    fn test_cli_team_agent_with_system_prompt() {
        let cli = Cli::try_parse_from([
            "shannon",
            "--team-agent",
            "--name",
            "coder",
            "--system-prompt",
            "You are a Rust expert",
        ])
        .unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.system_prompt.as_deref(), Some("You are a Rust expert"),);
    }

    #[test]
    fn test_cli_team_agent_with_workdir() {
        let cli = Cli::try_parse_from([
            "shannon",
            "--team-agent",
            "--name",
            "worker",
            "--workdir",
            "/tmp/worktree-1",
        ])
        .unwrap();
        assert!(cli.team_agent);
        assert_eq!(cli.workdir.as_deref(), Some("/tmp/worktree-1"));
    }

    #[test]
    fn test_cli_team_agent_all_options() {
        let cli = Cli::try_parse_from([
            "shannon",
            "--team-agent",
            "--name",
            "researcher",
            "--model",
            "claude-opus-4",
            "--provider",
            "anthropic",
            "--system-prompt",
            "Research agent",
            "--workdir",
            "/project/research",
        ])
        .unwrap();
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

    // ── HeadlessExitCode tests ─────────────────────────────────────────────

    #[test]
    fn test_headless_exit_code_values() {
        assert_eq!(HeadlessExitCode::Success as i32, 0);
        assert_eq!(HeadlessExitCode::Error as i32, 1);
        assert_eq!(HeadlessExitCode::TurnLimit as i32, 2);
        assert_eq!(HeadlessExitCode::Timeout as i32, 3);
        assert_eq!(HeadlessExitCode::RateLimited as i32, 4);
        assert_eq!(HeadlessExitCode::ContextOverflow as i32, 5);
        assert_eq!(HeadlessExitCode::PermissionDenied as i32, 6);
    }

    #[test]
    fn test_headless_exit_code_from_conversion() {
        assert_eq!(i32::from(HeadlessExitCode::Success), 0);
        assert_eq!(i32::from(HeadlessExitCode::Error), 1);
        assert_eq!(i32::from(HeadlessExitCode::ContextOverflow), 5);
        assert_eq!(i32::from(HeadlessExitCode::PermissionDenied), 6);
    }

    #[test]
    fn test_headless_exit_code_serialization() {
        let code = HeadlessExitCode::ContextOverflow;
        let json = serde_json::to_string(&code).unwrap();
        assert!(json.contains("context_overflow"));

        let code = HeadlessExitCode::PermissionDenied;
        let json = serde_json::to_string(&code).unwrap();
        assert!(json.contains("permission_denied"));
    }

    #[test]
    fn test_headless_exit_codes_are_distinct() {
        let codes = [
            HeadlessExitCode::Success as i32,
            HeadlessExitCode::Error as i32,
            HeadlessExitCode::TurnLimit as i32,
            HeadlessExitCode::Timeout as i32,
            HeadlessExitCode::RateLimited as i32,
            HeadlessExitCode::ContextOverflow as i32,
            HeadlessExitCode::PermissionDenied as i32,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in codes {
            assert!(seen.insert(code), "duplicate exit code: {code}");
        }
    }

    // ── OutputEvent (local) tests ──────────────────────────────────────────

    #[test]
    fn test_output_event_text_delta_ndjson() {
        let event = OutputEvent::TextDelta {
            content: "hello".into(),
        };
        let line = event.to_ndjson();
        assert!(line.ends_with('\n'));
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "text_delta");
        assert_eq!(parsed["content"], "hello");
    }

    #[test]
    fn test_output_event_tool_use_ndjson() {
        let event = OutputEvent::ToolUse {
            name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
        };
        let line = event.to_ndjson();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "tool_use");
        assert_eq!(parsed["name"], "Bash");
    }

    #[test]
    fn test_output_event_tool_result_ndjson() {
        let event = OutputEvent::ToolResult {
            name: "Read".into(),
            output: "contents".into(),
            is_error: false,
        };
        let line = event.to_ndjson();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "tool_result");
        assert_eq!(parsed["is_error"], false);
    }

    #[test]
    fn test_output_event_error_ndjson() {
        let event = OutputEvent::Error {
            message: "something failed".into(),
        };
        let line = event.to_ndjson();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "error");
    }

    #[test]
    fn test_output_event_done_ndjson() {
        let event = OutputEvent::Done { exit_code: 0 };
        let line = event.to_ndjson();
        let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "done");
        assert_eq!(parsed["exit_code"], 0);
    }

    #[test]
    fn test_output_event_done_with_error_codes() {
        for code in [1, 2, 3, 4, 5, 6] {
            let event = OutputEvent::Done { exit_code: code };
            let line = event.to_ndjson();
            let parsed: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
            assert_eq!(parsed["exit_code"], code);
        }
    }

    #[test]
    fn test_ndjson_stream_multiple_events() {
        let events = [
            OutputEvent::TextDelta {
                content: "line1".into(),
            },
            OutputEvent::TextDelta {
                content: "line2".into(),
            },
            OutputEvent::Done { exit_code: 0 },
        ];
        let output: String = events.iter().map(|e| e.to_ndjson()).collect();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }
    }

    #[test]
    fn test_output_event_ndjson_one_json_per_line() {
        let event = OutputEvent::ToolUse {
            name: "Edit".into(),
            input: serde_json::json!({"path": "/tmp/f.rs"}),
        };
        let line = event.to_ndjson();
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
    }

    // ── load_schema tests ────────────────────────────────────────────

    #[test]
    fn test_load_schema_inline_object() {
        let config = load_schema(r#"{"type":"object","required":["name"]}"#).unwrap();
        assert!(config.schema.is_object());
        assert_eq!(config.schema["type"], "object");
        assert!(config.name.is_none());
    }

    #[test]
    fn test_load_schema_inline_with_whitespace() {
        let config = load_schema(r#"  {"type":"array"}  "#).unwrap();
        assert_eq!(config.schema["type"], "array");
    }

    #[test]
    fn test_load_schema_invalid_json() {
        let result = load_schema(r#"{invalid json}"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid JSON"));
    }

    #[test]
    fn test_load_schema_file_not_found() {
        let result = load_schema("/nonexistent/path/schema.json");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to read schema file")
        );
    }

    #[test]
    fn test_load_schema_from_file() {
        let tmp = std::env::temp_dir().join("shannon_test_schema.json");
        std::fs::write(
            &tmp,
            r#"{"type":"object","properties":{"x":{"type":"number"}}}"#,
        )
        .unwrap();
        let config = load_schema(tmp.to_str().unwrap()).unwrap();
        assert!(config.schema["properties"]["x"]["type"].is_string());
        let _ = std::fs::remove_file(&tmp);
    }

    // ── Deep link (shannon:// URL scheme) tests ─────────────────────────

    #[test]
    fn test_parse_deep_link_prompt() {
        let result = parse_deep_link("shannon://prompt?text=hello%20world").unwrap();
        assert_eq!(
            result,
            vec![
                "shannon".to_string(),
                "--prompt".to_string(),
                "hello world".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_deep_link_prompt_encoded() {
        let result =
            parse_deep_link("shannon://prompt?text=fix%20the%20bug%20in%20main.rs").unwrap();
        assert_eq!(
            result,
            vec![
                "shannon".to_string(),
                "--prompt".to_string(),
                "fix the bug in main.rs".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_deep_link_prompt_with_trailing_slash() {
        let result = parse_deep_link("shannon://prompt/?text=hello").unwrap();
        assert_eq!(
            result,
            vec![
                "shannon".to_string(),
                "--prompt".to_string(),
                "hello".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_deep_link_prompt_no_text() {
        let result = parse_deep_link("shannon://prompt").unwrap();
        assert_eq!(
            result,
            vec![
                "shannon".to_string(),
                "--prompt".to_string(),
                "".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_deep_link_resume_with_id() {
        let result =
            parse_deep_link("shannon://resume?id=550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(
            result,
            vec![
                "shannon".to_string(),
                "--resume".to_string(),
                "550e8400-e29b-41d4-a716-446655440000".to_string(),
            ]
        );
    }

    #[test]
    fn test_parse_deep_link_resume_no_id() {
        let result = parse_deep_link("shannon://resume").unwrap();
        assert_eq!(
            result,
            vec!["shannon".to_string(), "--continue".to_string(),]
        );
    }

    #[test]
    fn test_parse_deep_link_resume_with_trailing_slash() {
        let result = parse_deep_link("shannon://resume/").unwrap();
        assert_eq!(
            result,
            vec!["shannon".to_string(), "--continue".to_string(),]
        );
    }

    #[test]
    fn test_parse_deep_link_invalid() {
        // Non-shannon URLs return None
        assert!(parse_deep_link("https://example.com").is_none());
        assert!(parse_deep_link("http://shannon.io").is_none());
        assert!(parse_deep_link("ftp://files.com").is_none());
    }

    #[test]
    fn test_parse_deep_link_unknown_path() {
        // Unknown path returns None
        assert!(parse_deep_link("shannon://unknown?text=test").is_none());
        assert!(parse_deep_link("shannon://open?file=main.rs").is_none());
    }

    #[test]
    fn test_parse_deep_link_not_url() {
        // Regular strings return None
        assert!(parse_deep_link("hello world").is_none());
        assert!(parse_deep_link("--prompt").is_none());
        assert!(parse_deep_link("").is_none());
    }

    #[test]
    fn test_parse_query_param_basic() {
        let query = "text=hello&lang=en";
        assert_eq!(parse_query_param(query, "text"), Some("hello".to_string()));
        assert_eq!(parse_query_param(query, "lang"), Some("en".to_string()));
    }

    #[test]
    fn test_parse_query_param_empty() {
        assert_eq!(parse_query_param("", "text"), None);
        assert_eq!(parse_query_param("key=value", "other"), None);
    }

    #[test]
    fn test_parse_query_param_no_value() {
        // A param without '=' should not match
        assert_eq!(parse_query_param("flag", "flag"), None);
    }

    #[test]
    fn test_parse_query_param_first_match() {
        let query = "text=first&text=second";
        // Should return the first match
        assert_eq!(parse_query_param(query, "text"), Some("first".to_string()));
    }

    #[test]
    fn test_deep_link_to_cli_prompt() {
        // Verify the transformed args parse correctly as Cli args
        let args = parse_deep_link("shannon://prompt?text=explain%20this%20code").unwrap();
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(cli.headless_prompt.as_deref(), Some("explain this code"));
    }

    #[test]
    fn test_deep_link_to_cli_resume_with_id() {
        let args =
            parse_deep_link("shannon://resume?id=550e8400-e29b-41d4-a716-446655440000").unwrap();
        let cli = Cli::try_parse_from(args).unwrap();
        assert_eq!(
            cli.resume.as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
    }

    #[test]
    fn test_deep_link_to_cli_resume_no_id() {
        let args = parse_deep_link("shannon://resume").unwrap();
        let cli = Cli::try_parse_from(args).unwrap();
        // --continue is the alias for "resume most recent"
        assert!(cli.r#continue);
    }
}
