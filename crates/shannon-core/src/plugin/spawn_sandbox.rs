//! Execution-world boundary carried by a plugin's stdio spawn chain
//! (write_files enforcement — the closing of the §4.9 scaffolding).
//!
//! A manifest that declares `write_files` gets its server processes spawned
//! **inside a manifest-derived execution world**: the permission semantics
//! (`declaration = allow-set`) stop being an in-process promise and become an
//! OS-level fact. The boundary itself is platform-pluggable and crosses the
//! core boundary as this one enum:
//!
//! - [`PluginSpawnGuard::ForkInit`] — a [`ChildWorldInit`] installed between
//!   fork and exec on every spawn (Linux Landlock; the §4.12 execution
//!   world). A failed install aborts the spawn — a child never runs without
//!   its boundary (fail-closed).
//! - [`PluginSpawnGuard::Seatbelt`] — the existing argv-level bridge
//!   ([`SandboxExecutor`]); the spawn is rewritten to run under
//!   `sandbox-exec` with a profile generated from the same policy (macOS).
//!
//! Everything else about the spawn — argv, env, streams, exit codes — is
//! preserved byte-for-byte. A `None` guard (undeclared / non-write_files
//! manifests, or a degraded backend) skips all of this: no hook, no rewrite,
//! the exact legacy spawn path.

use crate::sandbox::SandboxExecutor;
use shannon_tool_interface::sandbox::ChildWorldInit;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::process::Command;

/// The execution-world boundary Shannon installs around a write_files-declaring
/// plugin's stdio server spawns.
///
/// Built by the sandbox assembly (`shannon-tools`) from the policy derived by
/// [`PluginPermissionPolicy::spawn_sandbox_policy`](super::PluginPermissionPolicy::spawn_sandbox_policy);
/// consumed by `discover_tools_guarded` / `McpToolAdapter` at both spawn
/// points (discovery + per-call cold spawn).
pub enum PluginSpawnGuard {
    /// Fork-time execution world: `init` runs inside each freshly forked
    /// child before exec. (Linux Landlock backend.)
    ForkInit(Arc<dyn ChildWorldInit>),
    /// argv-level bridge: spawns are rewritten through the executor
    /// (`sandbox-exec -p <profile> -- <program> <args…>`). (macOS Seatbelt.)
    Seatbelt(Arc<SandboxExecutor>),
}

impl std::fmt::Debug for PluginSpawnGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSpawnGuard")
            .field("kind", &self.kind())
            .finish()
    }
}

impl PluginSpawnGuard {
    /// Backend identifier for logs/events (`"landlock"` / `"seatbelt"`).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ForkInit(_) => "landlock",
            Self::Seatbelt(_) => "seatbelt",
        }
    }

    /// Pure prep step: returns the effective `(program, args)` to spawn.
    ///
    /// Fork-based boundaries are identity here (their enforcement installs at
    /// [`Self::install_fork_init`]); the Seatbelt bridge rewrites the argv to
    /// run under `sandbox-exec`, preserving the supplied env through the
    /// wrapper (the caller re-applies the same env afterwards, so this stays
    /// a pure computation over the requested spawn).
    pub fn prepare_program_args(
        &self,
        program: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<(String, Vec<String>), String> {
        match self {
            Self::ForkInit(_) => Ok((program.to_string(), args.to_vec())),
            Self::Seatbelt(executor) => {
                let mut cmd = std::process::Command::new(program);
                cmd.args(args);
                for (key, value) in env {
                    cmd.env(key, value);
                }
                executor.wrap_command(&mut cmd).map_err(|e| e.to_string())?;
                let program = cmd.get_program().to_string_lossy().into_owned();
                let args: Vec<String> = cmd
                    .get_args()
                    .map(|a| a.to_string_lossy().into_owned())
                    .collect();
                Ok((program, args))
            }
        }
    }

    /// Install the fork-time boundary onto the tokio command.
    ///
    /// The initializer runs inside the freshly forked child immediately
    /// before exec; an `Err` from it aborts the spawn, so a child can never
    /// start without its boundary. No-op for argv-level guards.
    pub fn install_fork_init(&self, cmd: &mut Command) {
        match self {
            Self::ForkInit(init) => {
                #[cfg(unix)]
                {
                    let init = Arc::clone(init);
                    // Mirrors `LocalProcess::install_fork_init_tokio`: unsafe
                    // is scoped to the pre_exec registration itself; the
                    // closure body is the backend's fail-closed installer.
                    unsafe {
                        cmd.pre_exec(move || init.init_child());
                    }
                }
                #[cfg(not(unix))]
                {
                    let _ = init;
                    let _ = cmd;
                }
            }
            Self::Seatbelt(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::path::PathBuf;

    /// A boundary that drops a marker file from inside the child, proving it
    /// ran pre-exec without disturbing the child's own work.
    struct MarkerInit(PathBuf);

    impl ChildWorldInit for MarkerInit {
        fn init_child(&self) -> io::Result<()> {
            std::fs::write(&self.0, b"installed")
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fork_init_guard_installs_world_inside_child() {
        let dir = tempfile::tempdir().expect("temp dir");
        let marker = dir.path().join("boundary-marker");
        let guard = PluginSpawnGuard::ForkInit(Arc::new(MarkerInit(marker.clone())));

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "echo child-ran"]);
        guard.install_fork_init(&mut cmd);

        let out = cmd.output().await.expect("spawn with boundary");
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "child-ran");
        assert_eq!(
            std::fs::read(&marker).expect("marker written pre-exec"),
            b"installed"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fork_init_guard_failure_aborts_spawn_fail_closed() {
        struct RefusingInit;
        impl ChildWorldInit for RefusingInit {
            fn init_child(&self) -> io::Result<()> {
                Err(io::Error::from_raw_os_error(13)) // EACCES
            }
        }
        let guard = PluginSpawnGuard::ForkInit(Arc::new(RefusingInit));
        let mut cmd = Command::new("/bin/true");
        guard.install_fork_init(&mut cmd);
        let err = cmd
            .output()
            .await
            .expect_err("boundary failure must abort the spawn");
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    /// Fork-based boundaries never touch the argv: the legacy spawn shape is
    /// preserved exactly (compat contract for the derivation-vs-application
    /// split).
    #[test]
    fn prepare_program_args_is_identity_for_fork_guards() {
        let dir = tempfile::tempdir().expect("temp dir");
        let guard = PluginSpawnGuard::ForkInit(Arc::new(MarkerInit(dir.path().join("m"))));
        let args = vec!["-c".to_string(), "echo hi".to_string()];
        let got = guard
            .prepare_program_args("/bin/sh", &args, &HashMap::new())
            .expect("identity prep");
        assert_eq!(got.0, "/bin/sh");
        assert_eq!(got.1, args);
        assert_eq!(guard.kind(), "landlock");
    }

    /// Seatbelt bridge rewrites through the executor — pinned on macOS only,
    /// where `sandbox-exec` is the auto-detected backend.
    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_guard_wraps_argv_through_sandbox_exec() {
        use crate::sandbox::{NetworkAccess, SandboxConfig};
        let workspace = tempfile::tempdir().expect("temp dir");
        let executor = Arc::new(SandboxExecutor::new(
            SandboxConfig::new(workspace.path()).with_network(NetworkAccess::Full),
        ));
        if executor.sandbox_type() != crate::sandbox::SandboxType::Seatbelt {
            println!("sandbox-exec unavailable; skipping");
            return;
        }
        let guard = PluginSpawnGuard::Seatbelt(executor);
        let mut env = HashMap::new();
        env.insert("PROBE".to_string(), "1".to_string());
        let (program, _args) = guard
            .prepare_program_args("/bin/echo", &["hi".to_string()], &env)
            .expect("seatbelt prep");
        assert_eq!(program, "/usr/bin/sandbox-exec");
        assert_eq!(guard.kind(), "seatbelt");
    }
}
