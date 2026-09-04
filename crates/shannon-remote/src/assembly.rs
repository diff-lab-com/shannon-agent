//! Session-world assembly helpers shared by the REPL and headless CLI.
//!
//! Two shapes:
//! - **Dynamic** (interactive REPL): a [`DynamicWorld`] decorator starts in
//!   the local world and can be switched to an SSH/Docker target at runtime
//!   via `/remote use`. The shared [`WorldSandboxHandle`] lets the swap
//!   retarget every registered tool's path sandbox without a registry
//!   rebuild.
//! - **Static** (headless `--target`): the world is fixed for the process
//!   lifetime, so the target's providers are assembled directly with the
//!   remote workspace as the sandbox root.

use std::sync::Arc;

use crate::dynamic::{DynamicWorld, WorldState};
use crate::target::{RemoteTarget, RemotesFile, TargetKind};
use crate::{docker, ssh};
use shannon_tools::ToolProviders;

/// Everything a REPL needs to keep a swappable world alive.
pub struct DynamicAssembly {
    pub providers: ToolProviders,
    pub world: Arc<DynamicWorld>,
    pub state: Arc<WorldState>,
    pub world_sandbox: Arc<shannon_tools::file::sandbox::WorldSandboxHandle>,
}

/// Dynamic assembly for interactive sessions: starts local, switchable.
pub fn assemble_dynamic() -> DynamicAssembly {
    let defaults = ToolProviders::default();
    let (world, state) = DynamicWorld::new(defaults.fs.clone(), defaults.process.clone());
    let world_sandbox = Arc::new(shannon_tools::file::sandbox::WorldSandboxHandle::new());
    let providers = ToolProviders {
        fs: world.clone(),
        process: world.clone(),
        denial_classifier: None,
        world_sandbox: Some(world_sandbox.clone()),
    };
    DynamicAssembly {
        providers,
        world,
        state,
        world_sandbox,
    }
}

/// Resolve the session target: explicit CLI value first, then
/// `SHANNON_TARGET`, then `default_target` from `~/.shannon/remotes.toml`.
pub fn resolve_session_target(explicit: Option<&str>) -> Option<RemoteTarget> {
    RemotesFile::load_default().resolve_active(explicit)
}

/// Point `DynamicWorld` (and the shared sandbox roots) at `target`.
///
/// Returns the health report on success; on failure the local world stays
/// installed. `home` from the health check feeds the sandbox's home-boundary
/// check so remote absolute paths validate correctly.
pub async fn connect_dynamic(
    assembly: &DynamicAssembly,
    target: &RemoteTarget,
) -> std::io::Result<ssh::HealthReport> {
    let health = assembly.world.connect_target(target).await?;
    assembly
        .world_sandbox
        .set(shannon_tools::file::sandbox::WorldRoots {
            allowed_roots: vec![target.workspace_dir.clone()],
            home_dir: Some(std::path::PathBuf::from(&health.home)),
        });
    Ok(health)
}

/// Return the dynamic world to the local machine (clears sandbox overrides).
pub fn disconnect_dynamic(assembly: &DynamicAssembly) {
    assembly.world.disconnect();
    assembly
        .world_sandbox
        .set(shannon_tools::file::sandbox::WorldRoots::default());
}

/// Headless assembly inputs: a remote world when a target resolves
/// (`SHANNON_TARGET`/`default_target`), otherwise the local project dir with
/// default providers.
pub async fn assemble_for_headless() -> std::io::Result<(std::path::PathBuf, ToolProviders)> {
    match resolve_session_target(None) {
        Some(target) => {
            tracing::info!(target = %target.name, kind = %target.kind, "remote target active");
            let providers = assemble_static(&target).await?;
            Ok((target.workspace_dir.clone(), providers))
        }
        None => {
            let project_dir = std::env::current_dir().unwrap_or_default();
            Ok((project_dir, ToolProviders::default()))
        }
    }
}

/// Static assembly for headless runs: connect once, no runtime switching.
/// The remote workspace becomes the (only) sandbox root.
pub async fn assemble_static(target: &RemoteTarget) -> std::io::Result<ToolProviders> {
    match target.kind {
        TargetKind::Ssh => {
            let runtime = ssh::SshRuntime::connect(target).await?;
            let health = runtime.health().await?;
            if !health.workspace_exists {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "workspace_dir {} does not exist on {}",
                        target.workspace_dir.display(),
                        target.ssh_destination()
                    ),
                ));
            }
            // Process world first (borrows the runtime), then the SFTP file
            // world attaches to the same connection.
            let process = ssh::SshProcess::new(runtime.clone(), target.workspace_dir.clone());
            let fs = ssh::SshFs::connect(runtime).await?;
            Ok(ToolProviders {
                fs,
                process,
                denial_classifier: None,
                world_sandbox: None,
            })
        }
        TargetKind::Docker => {
            let process = docker::DockerExecProcess::new(
                target.container.clone().unwrap_or_default(),
                target.workspace_dir.clone(),
                std::sync::Arc::new(shannon_core::providers::LocalProcess::new()),
            );
            let fs = docker::DockerExecFs::new(process.clone());
            Ok(ToolProviders {
                fs,
                process,
                denial_classifier: None,
                world_sandbox: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_assembly_starts_local_with_handle() {
        let assembly = assemble_dynamic();
        assert_eq!(assembly.state.status(), ssh::WorldStatus::Local);
        assert!(!assembly.world.is_remote());
        // Empty override = passthrough (configured/local roots govern).
        assert!(assembly.world_sandbox.current().is_none());
    }
}
