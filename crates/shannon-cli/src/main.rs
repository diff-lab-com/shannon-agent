use anyhow::Result;
use clap::{Parser, Subcommand};
use shannon_ui::Repl;

/// Shannon Code - Claude Code in Rust
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Repl { file } => {
            let mut repl = Repl::new().map_err(|e| anyhow::anyhow!("{:?}", e))?;
            repl.run().map_err(|e| anyhow::anyhow!("{:?}", e))?;
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
                println!("Config: {}", key);
            } else {
                println!("Show all config");
            }
        }
    }

    Ok(())
}
