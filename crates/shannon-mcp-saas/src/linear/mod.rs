//! Linear SaaS module.
//!
//! Mirrors the `slack/` and `jira/` layouts. Linear uses GraphQL +
//! personal API keys (no OAuth); the auth surface is correspondingly
//! thin (just a `TokenProvider`). See:
//!
//! - [`auth`] — Token + keyring + env var fallback.
//! - [`api`]  — GraphQL client with retry/backoff.
//! - [`tools`] — 5 MCP tools + ServerTool adapter.

pub mod api;
pub mod auth;
pub mod tools;

#[cfg(test)]
mod tests;

/// Public constant for cross-cutting references (e.g. logging at the
/// binary level). Mirrors `slack::SLACK_SAAS` style used elsewhere.
pub const LINEAR: &str = "linear";
