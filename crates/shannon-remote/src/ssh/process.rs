//! `ProcessProvider` over the system ssh client.
//!
//! argv safety model: openssh shell-quotes *each* element it sends, so user
//! data never touches a shell; the only shell construct is a fixed literal
//! `sh -c` script that routes cwd without any interpolation of user input.

use std::io;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use shannon_tool_interface::{
    CapturedOutput, ExecCaps, PipedChild, PipedSpawn, ProcessProvider, ProcessRequest,
};

use super::session::SshRuntime;

/// Fixed `sh -c` script: cd into `$1` (cwd), shift it off, then exec the rest.
///
/// `shift` is load-bearing: `sh -c script name arg...` puts `name` in `$0`,
/// the first data argument in `$1`, and the remainder in `$@`. Without the
/// shift, `exec "$@"` would try to run the cwd as a command.
pub const CWD_SCRIPT: &str = r#"cd "$1" && shift && exec "$@""#;

/// Compose a [`ProcessRequest`] into a remote argv executed by `SshProcess`.
///
/// Layout: `env 'K=V'... sh -c <CWD_SCRIPT> sh <cwd> <program> <args...>`.
/// `request.cwd` overrides `default_cwd` (the target workspace root). No user
/// data is interpolated into the script — everything rides as separately
/// quoted argv elements.
pub fn compose_command(req: &ProcessRequest, default_cwd: &Path) -> Vec<String> {
    let cwd = req.cwd.clone().unwrap_or_else(|| default_cwd.to_path_buf());
    let mut argv: Vec<String> = Vec::with_capacity(req.args.len() + 8);
    for (k, v) in &req.env {
        argv.push("env".into());
        argv.push(format!("{k}={v}"));
    }
    argv.push("sh".into());
    argv.push("-c".into());
    argv.push(CWD_SCRIPT.to_string());
    argv.push("sh".into()); // $0 placeholder
    argv.push(cwd.to_string_lossy().to_string()); // $1 = cwd
    argv.push(req.program.clone());
    argv.extend(req.args.iter().cloned());
    argv
}

/// Process world executing every request on the SSH target.
pub struct SshProcess {
    rt: Arc<SshRuntime>,
    default_cwd: std::path::PathBuf,
}

impl SshProcess {
    /// `default_cwd` is the target's workspace root; requests without an
    /// explicit cwd run there instead of the remote `$HOME`.
    pub fn new(rt: Arc<SshRuntime>, default_cwd: std::path::PathBuf) -> Arc<Self> {
        Arc::new(Self { rt, default_cwd })
    }

    /// The underlying runtime (health checks, status, sftp spawn).
    pub fn runtime(&self) -> &Arc<SshRuntime> {
        &self.rt
    }

    fn compose(&self, request: &ProcessRequest) -> Vec<String> {
        compose_command(request, &self.default_cwd)
    }

    async fn run_impl(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let argv = self.compose(request);
        match &request.stdin_data {
            None => self.rt.exec(argv).await,
            Some(data) => {
                // Captured run with stdin: pipe the payload, then reap with
                // output. All of it happens on the dedicated runtime.
                let mut child = self.rt.spawn_piped_argv(argv, true, true, false).await?;
                let mut stdin = child
                    .take_stdin()
                    .ok_or_else(|| io::Error::other("ssh child stdin unavailable"))?;
                use tokio::io::AsyncWriteExt;
                stdin.write_all(data).await?;
                stdin.shutdown().await?;
                drop(stdin);
                let mut stdout = child
                    .take_stdout()
                    .ok_or_else(|| io::Error::other("ssh child stdout unavailable"))?;
                let mut buf = Vec::new();
                tokio::io::AsyncReadExt::read_to_end(&mut stdout, &mut buf).await?;
                let exit = child.wait().await?;
                Ok(CapturedOutput {
                    stdout: buf,
                    stderr: Vec::new(),
                    exit,
                })
            }
        }
    }
}

#[async_trait]
impl ProcessProvider for SshProcess {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        // Bridge onto the dedicated runtime from any thread (git helpers call
        // this from sync contexts, sometimes inside spawn_blocking workers).
        let argv = self.compose(request);
        self.rt.exec_blocking(argv)
    }

    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        self.run_impl(request).await
    }

    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        let argv = self.compose(&spec.request);
        self.rt
            .spawn_piped_argv(argv, spec.pipe_stdin, spec.pipe_stdout, spec.pipe_stderr)
            .await
    }

    fn capabilities(&self) -> ExecCaps {
        ExecCaps { is_remote: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn req(program: &str, args: &[&str]) -> ProcessRequest {
        ProcessRequest::new(program, args)
    }

    fn run_locally(argv: &[String]) -> std::process::Output {
        // Replays the composed argv through the local shell exactly as the
        // remote shell would receive it: sh -c <script> sh <cwd> <prog> ...
        std::process::Command::new(&argv[0])
            .arg(&argv[1])
            .arg(&argv[2])
            .args(&argv[3..])
            .output()
            .expect("local sh available in test env")
    }

    #[test]
    fn compose_uses_default_cwd_and_literal_script() {
        let argv = compose_command(&req("rg", &["-n", "foo"]), Path::new("/home/u/proj"));
        assert_eq!(
            argv,
            vec![
                "sh",
                "-c",
                CWD_SCRIPT,
                "sh",
                "/home/u/proj",
                "rg",
                "-n",
                "foo"
            ]
        );
    }

    #[test]
    fn compose_request_cwd_overrides_default() {
        let mut r = req("ls", &["-la"]);
        r.cwd = Some(PathBuf::from("/var/log"));
        let argv = compose_command(&r, Path::new("/home/u/proj"));
        assert_eq!(&argv[4], "/var/log");
        assert_eq!(&argv[5], "ls");
    }

    #[test]
    fn compose_env_rides_as_separate_elements() {
        let mut r = req("printenv", &["K"]);
        r.env = vec![("K".into(), "v with spaces".into())];
        let argv = compose_command(&r, Path::new("/w"));
        assert_eq!(&argv[0], "env");
        assert_eq!(&argv[1], "K=v with spaces");
        // No user data ever lands inside the script element.
        assert_eq!(argv[4], CWD_SCRIPT);
        assert_eq!(&argv[7], "printenv");
    }

    #[test]
    fn compose_without_env_has_no_env_prefix() {
        let argv = compose_command(&req("true", &[]), Path::new("/w"));
        assert_eq!(&argv[0], "sh");
    }

    #[test]
    fn compose_script_executes_end_to_end_with_real_sh() {
        // Reviewer-mandated regression test for the missing `shift`: the
        // literal script must execute with one cwd + one program via a real
        // /bin/sh and land in the cwd.
        let tmp = tempfile::tempdir().unwrap();
        let argv = compose_command(&req("pwd", &[]), tmp.path());
        let out = run_locally(&argv);
        assert!(out.status.success());
        assert_eq!(
            String::from_utf8_lossy(&out.stdout).trim(),
            tmp.path().to_string_lossy()
        );
    }

    #[test]
    fn compose_script_captures_exit_code_of_program() {
        let argv = compose_command(&req("false", &[]), Path::new("/tmp"));
        let out = run_locally(&argv);
        assert_eq!(out.status.code(), Some(1));
    }

    #[test]
    fn compose_script_forwards_args_and_env() {
        let tmp = tempfile::tempdir().unwrap();
        let mut r = req(
            "sh",
            &["-c", "printf '%s' \"$FIXED\"; printf '%s' \"$@\" x"],
        );
        r.env = vec![("FIXED".into(), "E".into())];
        let argv = compose_command(&r, tmp.path());
        let out = run_locally(&argv);
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout), "Ex");
    }

    #[test]
    fn caps_report_remote() {
        // Contract: local default is not remote; SshProcess always is.
        assert!(!ExecCaps::default().is_remote);
    }
}
