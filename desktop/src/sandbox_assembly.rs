//! P1-3: desktop command-sandbox assembly.
//!
//! The desktop `sandbox.mode` config key (`off` | `local` | `landlock` —
//! the same vocabulary as the engine's `[sandbox]` TOML table /
//! `SHANNON_SANDBOX` env var) decorates the execution worlds **at tool
//! assembly time**, exactly where the TUI's flag is consumed by
//! `register_default_tools_with_project_dir_ex`. Changing the mode takes
//! effect on the next app launch (the registry is built once at startup),
//! mirroring the TUI env-flag behavior.
//!
//! Tier mapping (frozen for the report):
//!
//! | config value | UI tier              | world                                             |
//! |--------------|----------------------|---------------------------------------------------|
//! | `off`/unset  | 关闭                  | passthrough — the dynamic world, byte-identical    |
//! | `local`      | 只读文件系统           | user-space fs policy over the dynamic world        |
//! | `landlock`   | 完全 (experimental)    | kernel-enforced world (`shannon_tools::sandbox`)   |

use shannon_tool_interface::sandbox::SandboxMode;
use shannon_tools::ToolProviders;
use shannon_tools::sandbox::{
    SandboxSettings, SandboxedFs, SandboxedProcess, assemble, seed_policy,
};

/// Resolve the execution-world providers for a configured sandbox mode.
///
/// - `Ok(None)` → keep `base` unchanged (off / unset / empty).
/// - `Ok(Some(providers))` → use the sandboxed set instead of `base`.
/// - `Err(msg)` → invalid config token or the kernel backend failed to
///   assemble; callers must degrade **loudly** (log + fall back to `base`),
///   never silently pretend to restrict.
pub fn effective_sandbox_providers(
    mode: Option<&str>,
    working_dir: Option<&str>,
    base: &ToolProviders,
) -> Result<Option<ToolProviders>, String> {
    let Some(mode) = mode.map(str::trim).filter(|m| !m.is_empty()) else {
        return Ok(None);
    };
    let sandbox_mode = match mode {
        "off" => return Ok(None),
        "local" => SandboxMode::Local,
        "landlock" => SandboxMode::Landlock,
        other => {
            return Err(format!(
                "unknown sandbox.mode `{other}` (expected off|local|landlock)"
            ));
        }
    };

    let project_dir = working_dir
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let settings = SandboxSettings {
        mode: sandbox_mode,
        ..SandboxSettings::default()
    };

    match sandbox_mode {
        SandboxMode::Local => {
            // User-space enforcement: the in-process fs tools get the policy
            // mirror; child processes are NOT kernel-restricted (same
            // semantics as `assemble_local`, but layered over the dynamic
            // world instead of replacing it).
            let policy = std::sync::Arc::new(seed_policy(&project_dir, &settings));
            Ok(Some(ToolProviders {
                fs: std::sync::Arc::new(SandboxedFs::new(base.fs.clone(), policy)),
                process: std::sync::Arc::new(SandboxedProcess::new(
                    base.process.clone(),
                    "local",
                    Vec::new(),
                )),
                denial_classifier: None,
                world_sandbox: base.world_sandbox.clone(),
            }))
        }
        SandboxMode::Landlock => {
            // Kernel-enforced world. Degrades with a loud error (surfaced to
            // the caller) rather than a silent fake sandbox.
            match assemble(&settings, &project_dir) {
                Ok(assembled) => {
                    for notice in &assembled.notices {
                        tracing::warn!(tag = %notice.tag, "sandbox: {}", notice.detail);
                    }
                    Ok(Some(assembled.providers))
                }
                Err(e) => Err(format!("sandbox=landlock unavailable: {e}")),
            }
        }
        SandboxMode::Off => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ToolProviders {
        shannon_remote::assembly::assemble_dynamic().providers
    }

    #[test]
    fn unset_and_off_keep_base_providers() {
        assert!(
            effective_sandbox_providers(None, None, &base())
                .unwrap()
                .is_none()
        );
        assert!(
            effective_sandbox_providers(Some("off"), None, &base())
                .unwrap()
                .is_none()
        );
        assert!(
            effective_sandbox_providers(Some("  "), None, &base())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn unknown_mode_is_a_loud_error() {
        // ToolProviders isn't Debug, so go through `.err()` instead of
        // `unwrap_err()`.
        let err = effective_sandbox_providers(Some("banana"), None, &base())
            .err()
            .expect("unknown mode must be an error");
        assert!(err.contains("unknown sandbox.mode"), "{err}");
    }

    #[test]
    fn local_mode_decorates_fs_and_process_over_the_dynamic_world() {
        let unwrapped = base();
        let providers =
            effective_sandbox_providers(Some("local"), Some("/tmp"), &unwrapped).unwrap();
        let providers = providers.expect("local mode must yield providers");
        // The dynamic world is decorated, not replaced: the world_sandbox
        // handle (remote-attach seam) is preserved and the fs/process worlds
        // are wrapped policy decorators.
        assert!(providers.world_sandbox.is_some(), "world_sandbox preserved");
        // The seeded policy scopes writes to the working dir: the same math
        // the SandboxedFs decorator enforces for the fs tools.
        let settings = SandboxSettings {
            mode: SandboxMode::Local,
            ..SandboxSettings::default()
        };
        let policy = seed_policy(std::path::Path::new("/tmp"), &settings);
        assert!(policy.allows_write(std::path::Path::new("/tmp/proj/file")));
        assert!(!policy.allows_write(std::path::Path::new("/etc/passwd")));
    }

    #[test]
    fn landlock_mode_either_assembles_or_fails_loudly() {
        // Kernel support is platform-dependent; both outcomes are valid, but
        // a failure must be an explicit Err (never a silent passthrough).
        match effective_sandbox_providers(Some("landlock"), Some("/tmp"), &base()) {
            Ok(Some(_)) => {} // kernel world assembled
            Ok(None) => panic!("landlock must not degrade silently"),
            Err(msg) => assert!(msg.contains("landlock"), "{msg}"),
        }
    }
}
