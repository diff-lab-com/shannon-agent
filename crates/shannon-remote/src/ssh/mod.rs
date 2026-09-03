//! SSH execution world over the system ssh client.
//!
//! Reuses `~/.ssh/config`, ssh-agent and known_hosts wholesale — Shannon
//! stores no keys and performs no host-key management of its own (TOFU via
//! `StrictHostKeyChecking=accept-new`).

pub mod discover;
pub mod fs;
pub mod process;
pub mod session;

pub use discover::{discover_ssh_hosts, SshHostCandidate};
pub use fs::SshFs;
pub use process::{compose_command, SshProcess};
pub use session::{HealthReport, SshRuntime, WorldStatus};
