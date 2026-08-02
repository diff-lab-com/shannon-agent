//! GitHub SaaS module. Spike skeleton — canned responses only.
//!
//! The four-file split (`auth.rs`, `api.rs`, `tools.rs`, `tests.rs`) is
//! the template that Slack/Jira/Notion/Linear will reuse in step 3+.

pub mod api;
pub mod auth;
pub mod tools;

#[cfg(test)]
mod tests;
