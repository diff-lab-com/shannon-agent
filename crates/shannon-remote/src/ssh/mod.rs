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

/// Build the ssh target for ignored integration tests from the environment
/// (`SHANNON_TEST_SSH_HOST/_PORT/_USER/_WORKSPACE`; defaults localhost:22).
#[cfg(test)]
pub(crate) fn test_ssh_target() -> crate::target::RemoteTarget {
    use crate::target::{RemoteTarget, TargetKind};
    let host = std::env::var("SHANNON_TEST_SSH_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("SHANNON_TEST_SSH_PORT")
        .ok()
        .and_then(|p| p.parse().ok());
    let user = std::env::var("SHANNON_TEST_SSH_USER").ok();
    RemoteTarget {
        name: "it".into(),
        kind: TargetKind::Ssh,
        host: Some(host),
        port,
        user,
        container: None,
        shell: None,
        ssh_target: None,
        workspace_dir: std::env::var("SHANNON_TEST_SSH_WORKSPACE")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir()),
    }
}
