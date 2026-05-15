//! PTY (Pseudo-Terminal) executor for interactive command support.
//!
//! Uses `portable-pty` to allocate a real terminal, enabling:
//! - Interactive commands (gcloud auth login, ssh, etc.)
//! - Colored output from programs that check `isatty()`
//! - Line-buffered output matching user expectations

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use shannon_core::sandbox::audit_shell_command;
use shannon_types::recover_lock;
use std::io::Read;

/// Check if a command matches obviously destructive patterns that should
/// never be executed, even with user confirmation.
fn is_destructive_command(cmd: &str) -> bool {
    let lower = cmd.to_ascii_lowercase();
    let destructive = [
        "rm -rf /",
        "rm -rf /*",
        "rm -rf ~",
        "rm -rf *",
        "mkfs.",
        "dd if=",
        ":(){ :|:& };:",
        "> /dev/sd",
        "chmod -R 777 /",
        "chown -R",
        "shred /",
    ];
    destructive.iter().any(|p| lower.contains(p))
}

/// Result of a PTY command execution.
#[derive(Debug, Clone)]
pub struct PtyOutput {
    pub stdout: String,
    pub exit_code: i32,
}

/// Execute a command in a PTY and capture the output.
///
/// This is a synchronous function designed to be called from
/// `tokio::task::spawn_blocking`.
pub fn execute_in_pty(
    command: &str,
    cwd: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
    timeout_ms: Option<u64>,
) -> Result<PtyOutput, String> {
    if is_destructive_command(command) {
        return Err("Command blocked: potentially destructive operation detected".to_string());
    }

    audit_shell_command(command);

    let pty_system = native_pty_system();

    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {e}"))?;

    let mut cmd = CommandBuilder::new("bash");
    cmd.arg("-c");
    cmd.arg(command);

    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }

    if let Some(env_vars) = env {
        for (key, value) in env_vars {
            cmd.env(key.clone(), value.clone());
        }
    }

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn PTY command: {e}"))?;

    // Drop the slave side so EOF propagates when the child exits
    drop(pair.slave);

    let output_buf: std::sync::Arc<std::sync::Mutex<Vec<u8>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let reader_buf = output_buf.clone();
    let master = pair.master;

    let _reader_thread = std::thread::spawn(move || {
        let mut reader = match master.try_clone_reader() {
            Ok(r) => r,
            Err(e) => {
                tracing::error!("Failed to clone PTY reader: {e}");
                return;
            }
        };
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if let Ok(mut locked) = reader_buf.lock() {
                        locked.extend_from_slice(&buf[..n]);
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for the child process with optional timeout
    

    if let Some(timeout) = timeout_ms {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.exit_code() as i32;
                    // Give reader a moment to drain
                    std::thread::sleep(std::time::Duration::from_millis(50));
                    let output = recover_lock(output_buf.lock());
                    let stdout = String::from_utf8_lossy(&output).to_string();
                    return Ok(PtyOutput {
                        stdout,
                        exit_code: code,
                    });
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        let output = recover_lock(output_buf.lock());
                        let stdout = String::from_utf8_lossy(&output).to_string();
                        return Err(format!(
                            "Command timed out after {timeout}ms\n{stdout}"
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(format!("Failed to wait for PTY process: {e}")),
            }
        }
    } else {
        let status = child.wait().map_err(|e| format!("PTY wait failed: {e}"))?;
        let code = status.exit_code() as i32;
        std::thread::sleep(std::time::Duration::from_millis(50));
        let output = recover_lock(output_buf.lock());
        let stdout = String::from_utf8_lossy(&output).to_string();
        Ok(PtyOutput {
            stdout,
            exit_code: code,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_simple_echo() {
        let result = execute_in_pty("echo hello", None, None, Some(5000));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.stdout.contains("hello"));
        assert_eq!(output.exit_code, 0);
    }

    #[test]
    fn test_pty_exit_code() {
        let result = execute_in_pty("exit 42", None, None, Some(5000));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.exit_code, 42);
    }

    #[test]
    fn test_pty_cwd() {
        let result = execute_in_pty("pwd", Some("/tmp"), None, Some(5000));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.stdout.contains("/tmp"));
    }

    #[test]
    fn test_pty_env() {
        let mut env = std::collections::HashMap::new();
        env.insert("SHANNON_TEST_VAR".to_string(), "test_value_123".to_string());
        let result = execute_in_pty("echo $SHANNON_TEST_VAR", None, Some(&env), Some(5000));
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.stdout.contains("test_value_123"));
    }

    #[test]
    fn test_pty_timeout() {
        let result = execute_in_pty("sleep 30", None, None, Some(500));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timed out"));
    }

    #[test]
    fn test_pty_isatty() {
        let result = execute_in_pty(
            "test -t 0 && echo IS_TTY || echo NOT_TTY",
            None,
            None,
            Some(5000),
        );
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.stdout.contains("IS_TTY"));
    }
}
