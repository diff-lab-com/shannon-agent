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

#[cfg(feature = "slack")]
use shannon_mcp_saas::slack;
use shannon_mcp_saas::{github, server};

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    let saas = std::env::args().nth(1).unwrap_or_default();
    match saas.as_str() {
        "github" | "" => {
            let tools = github::tools::as_server_tool(github::tools::all_tools_unauth());
            server::print_tool_listing(&tools);
            if let Err(e) = server::run_stdio(&tools).await {
                eprintln!("shannon-mcp-saas: stdio loop failed: {e}");
                return ExitCode::FAILURE;
            }
        }
        #[cfg(feature = "slack")]
        "slack" => {
            let tools = slack::tools::as_server_tool(slack::tools::all_tools_unauth());
            server::print_tool_listing(&tools);
            if let Err(e) = server::run_stdio(&tools).await {
                eprintln!("shannon-mcp-saas: stdio loop failed: {e}");
                return ExitCode::FAILURE;
            }
        }
        other => {
            eprintln!("shannon-mcp-saas: unknown SaaS '{other}' (supported: github, slack)");
            return ExitCode::from(2);
        }
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
