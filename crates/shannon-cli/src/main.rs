use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use shannon_ui::Repl;

/// Shannon Code - AI-powered code assistant in Rust
///
/// A production-grade AI agent harness reimplementation in Rust
#[derive(Parser, Debug)]
#[command(name = "shannon")]
#[command(author = "Shannon Code Contributors")]
#[command(version = "0.1.0")]
#[command(about = "AI-powered code assistant in Rust", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
}

/// Parse CLI env overrides into a Vec of (key, value) pairs.
/// Returns Err for malformed entries (missing '=' or empty key).
fn parse_cli_env(env: &[String]) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for pair in env {
        match pair.split_once('=') {
            Some((key, value)) => {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                if key.is_empty() {
                    return Err(format!("empty key in env override: {pair}"));
                }
                pairs.push((key, value));
            }
            None => return Err(format!("malformed env override (missing '='): {pair}")),
        }
    }
    Ok(pairs)
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Repl { file: _, env } => {
            // Parse and validate env overrides before spawning tokio runtime.
            // We store them as a Vec to pass into the runtime, avoiding unsafe
            // std::env::set_var which is unsound after threads exist.
            let env_overrides = parse_cli_env(&env)
                .map_err(|e| anyhow::anyhow!(e))?;

            // Set env vars while still single-threaded (before tokio::main or runtime spawn).
            // This is safe because no other threads exist at this point.
            for (key, value) in &env_overrides {
                // SAFETY: we are in the main function before any async runtime
                // or thread spawning has occurred.
                unsafe { std::env::set_var(key, value) };
            }

            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{e:?}"))?;
            repl.run().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        Commands::Version { verbose } => {
            println!("Shannon Code v0.1.0");
            if verbose {
                println!("Rust {}", env!("CARGO_PKG_RUST_VERSION"));
                println!("Features: mcp, multi-agent, tools");
            }
        }
        Commands::Config { setting } => {
            if let Some(key) = setting {
                println!("Config: {key}");
            } else {
                println!("Show all config");
            }
        }
    }

    Ok(())
}
