//! Jira (Atlassian Cloud) SaaS module.
//!
//! Mirrors the `slack/` layout: `auth.rs` (OAuth + API token + keyring),
//! `api.rs` (REST client with rate-limit handling), `tools.rs` (four MCP
//! tools), `tests.rs` (mockito-backed coverage). Auth decisions live
//! server-side in [`crate::server::SessionGrants`] — these tools no
//! longer re-attest `args.permission`.

pub mod api;
pub mod auth;
pub mod tools;

#[cfg(test)]
mod tests;
