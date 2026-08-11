//! Notion SaaS module.
//!
//! Mirrors the `slack/` and `jira/` layout: `auth.rs` (Bearer token +
//! keyring), `api.rs` (REST client with rate-limit handling), `tools.rs`
//! (six MCP tools), `tests.rs` (mockito-backed coverage). Notion uses
//! internal-integration tokens (no OAuth), so `auth.rs` is much simpler
//! than the Slack/Jira flows. Permission gating is enforced server-side
//! in `crate::server::SessionGrants`; these tools do not re-attest
//! `args.permission`.

pub mod api;
pub mod auth;
pub mod tools;

#[cfg(test)]
mod tests;
