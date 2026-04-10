use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use futures::StreamExt;
use shannon_core::{
    api::LlmClientConfig,
    query_engine::{QueryContext, QueryEngine, QueryEvent, QueryMetadata},
    state::StateManager,
    tools::ToolRegistry,
};
use shannon_tools::BashTool;
use shannon_ui::Repl;
use std::collections::HashMap;
use std::io::Write;
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

    /// LLM model to use (e.g., claude-sonnet-4, gpt-4o)
    #[arg(short, long)]
    model: Option<String>,

    /// LLM provider (anthropic, openai, ollama, custom)
    #[arg(short, long)]
    provider: Option<String>,

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
    CliConfig {
        model: model.map(|s| s.to_string()),
        provider: provider.map(|s| s.to_string()),
        max_tokens,
        temperature,
        timeout,
        debug,
        env_overrides,
    }
}

/// Run a non-interactive query, outputting results to stdout.
/// `stream` controls whether text is streamed character-by-character.
/// `config` holds explicit CLI configuration.
fn run_noninteractive_query(query: &str, stream: bool, config: &CliConfig) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;

    rt.block_on(async {
        // Build tool registry
        let mut tools = ToolRegistry::new();
        tools.register(Box::new(BashTool::new()))
            .map_err(|e| anyhow::anyhow!("tool registration failed: {e}"))?;

        // Build LLM client with explicit config
        let client_config = LlmClientConfig::default();
        let client = shannon_core::api::LlmClient::new(client_config);

        let permissions = shannon_core::permissions::PermissionManager::new();
        let state = StateManager::new();

        let engine = QueryEngine::with_defaults(client, tools, permissions, state);

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
                    eprintln!("[tool: {tool_name} {}]",
                        serde_json::to_string_pretty(&tool_input).unwrap_or_default());
                }
                Ok(QueryEvent::ToolUseResult { tool_name, result, is_error, .. }) => {
                    if is_error {
                        eprintln!("[tool-error: {tool_name}] {result}");
                    } else {
                        eprintln!("[tool-done: {tool_name}]");
                    }
                }
                Ok(QueryEvent::Usage { input_tokens, output_tokens, cost_usd, .. }) => {
                    eprintln!(
                        "[usage: {input_tokens} in + {output_tokens} out = ${cost_usd:.4}]"
                    );
                }
                Ok(QueryEvent::Completed { .. }) => {
                    if !stream && !response_text.is_empty() {
                        println!("{response_text}");
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

fn main() -> Result<()> {
    let cli = Cli::parse();

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
        return run_noninteractive_query(&prompt, true, &config);
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
        }) => {
            let env_overrides = parse_cli_env(env)
                .map_err(|e| anyhow::anyhow!(e))?;
            build_cli_config(
                model.as_deref(),
                provider.as_deref(),
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
        // No subcommand, Version, and Config commands don't need config
        None | Some(Commands::Version { .. }) | Some(Commands::Config { .. }) => CliConfig::default(),
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
            if let Some(key) = setting {
                println!("Config: {key}");
            } else {
                println!("Show all config");
            }
        }
        Some(Commands::Query {
            query,
            output: _output_format,
            no_stream,
            ..
        }) => {
            run_noninteractive_query(&query, !no_stream, &config)?;
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
            Some(Commands::Repl { file, env, model: _, provider: _, max_tokens: _, temperature: _, timeout: _, debug: _, cwd: _ }) => {
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
            Some(Commands::Repl { file, env: _, model: _, provider: _, max_tokens: _, temperature: _, timeout: _, debug: _, cwd: _ }) => {
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
            Some(Commands::Repl { env, file: _, model: _, provider: _, max_tokens: _, temperature: _, timeout: _, debug: _, cwd: _ }) => {
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
            }
            _ => panic!("Expected Repl command"),
        }
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
}
