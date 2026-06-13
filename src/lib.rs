// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]

pub mod config;
pub mod events;
pub mod mcp;

#[cfg(feature = "tauri")]
pub mod commands;

#[cfg(feature = "tauri")]
pub mod scheduled_commands;

#[cfg(feature = "tauri")]
pub mod lsp_commands;
