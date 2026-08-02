//! shannon-mcp-saas: SaaS MCP servers.
//!
//! Each SaaS (GitHub → Slack → Jira → Notion → Linear) lives in its own
//! sub-module under `src/<saas>/` and exposes tool implementations
//! registered into a JSON-RPC stdio server. Slack/Jira/Notion/Linear
//! (P1-3 v2–v5) reuse the same directory layout and the
//! `auth.rs` / `api.rs` / `tools.rs` / `tests.rs` shape.

pub mod github;
pub mod server;
