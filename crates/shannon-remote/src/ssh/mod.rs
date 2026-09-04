//! SSH execution world over the system ssh client.
//!
//! Reuses `~/.ssh/config`, ssh-agent and known_hosts wholesale — Shannon
//! stores no keys and performs no host-key management of its own (TOFU via
//! `StrictHostKeyChecking=accept-new`).

pub mod discover;
pub mod fs;
pub mod process;
pub mod session;

pub use discover::{SshHostCandidate, discover_ssh_hosts};
pub use fs::SshFs;
pub use process::{SshProcess, compose_command};
pub use session::{HealthReport, SshRuntime, WorldStatus};
