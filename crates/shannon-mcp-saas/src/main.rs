//! shannon-mcp-saas binary entry point.
//!
//! Usage: `shannon-mcp-saas github` — selects the GitHub SaaS sub-module.
//! Other SaaS names (slack, jira, notion, linear) will be added in step 2+.
//!
//! On startup we print the list of registered tool names, then enter the
//! stdio JSON-RPC loop. Stdio is the canonical MCP transport for local
//! servers; the host (Shannon CLI / desktop) spawns us and talks to us
//! over stdin/stdout.

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

use shannon_mcp_saas::{github, server};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    // argv[1] selects the SaaS. Only `github` is wired in this spike.
    let saas = std::env::args().nth(1).unwrap_or_default();
    match saas.as_str() {
        "github" | "" => {
            // Proceed with GitHub tools.
        }
        other => {
            eprintln!("shannon-mcp-saas: unknown SaaS '{other}' (supported: github)");
            return ExitCode::from(2);
        }
    }

    // Wire up a default unauthenticated tool set. Production callers
    // (the future `serve` subcommand) will swap in an authenticated
    // client after completing the OAuth flow.
    let tools = github::tools::all_tools_unauth();

    server::print_tool_listing(&tools);

    if let Err(e) = server::run_stdio(&tools).await {
        eprintln!("shannon-mcp-saas: stdio loop failed: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("shannon_mcp_saas=info,shannon_mcp=warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
