//! Local shell resolution for user-facing command execution.
//!
//! `sh -c` is the execution contract across Shannon (hooks, `!` commands,
//! triggered routines, MCP header commands). Stock Windows ships no `sh`,
//! where every such spawn dies with `NotFound` — [`local_shell`] picks
//! `cmd /C` there instead, probed once per process, so simple commands
//! (echo, redirects, `.bat` scripts) keep working. With Git Bash installed
//! the probe finds `sh` and the original semantics are preserved.

use std::sync::OnceLock;

static WINDOWS_HAS_SH: OnceLock<bool> = OnceLock::new();

/// Does `sh -c "exit 0"` run successfully? (Probed once; only consulted
/// on Windows.)
fn windows_has_sh() -> bool {
    *WINDOWS_HAS_SH.get_or_init(|| {
        std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// Whether `sh -c` is usable on this machine (probed once; only ever
/// `false` on Windows without Git Bash/WSL).
pub fn has_sh() -> bool {
    if cfg!(windows) {
        windows_has_sh()
    } else {
        true
    }
}

/// Program + args for executing `command` with the local shell.
///
/// Returns `("sh", ["-c", command])` everywhere, except on `sh`-less
/// Windows where it returns `("cmd", ["/C", command])`.
pub fn local_shell(command: &str) -> (String, Vec<String>) {
    if cfg!(windows) && !windows_has_sh() {
        (
            "cmd".to_string(),
            vec!["/C".to_string(), command.to_string()],
        )
    } else {
        (
            "sh".to_string(),
            vec!["-c".to_string(), command.to_string()],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_shell_matches_platform_contract() {
        let (program, args) = local_shell("echo hi");
        if cfg!(windows) && !windows_has_sh() {
            assert_eq!(program, "cmd");
            assert_eq!(args, vec!["/C".to_string(), "echo hi".to_string()]);
        } else {
            assert_eq!(program, "sh");
            assert_eq!(args, vec!["-c".to_string(), "echo hi".to_string()]);
        }
    }
}
