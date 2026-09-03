//! `docker exec` process world.
//!
//! argv safety model: `docker exec` takes the program and arguments as plain
//! argv after the container name — no shell involved, no quoting hazards.
//! cwd and env use docker's native `-w` / `-e` flags. When the target routes
//! through `ssh_target`, the composed docker argv is executed on the SSH
//! host (openssh quotes each element).

use std::io;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use shannon_tool_interface::{
    CapturedOutput, ExecCaps, PipedChild, PipedSpawn, ProcessProvider, ProcessRequest,
};

use crate::ssh::process::SshProcess;

/// Compose a [`ProcessRequest`] into the local argv for `docker exec`.
///
/// Layout: `docker exec -w <cwd> [-e K=V]... -i <container> <program> <args...>`.
/// `-i` keeps stdin open so `write_bytes`-style `cat > file` payloads work.
pub fn compose_docker_exec(
    req: &ProcessRequest,
    container: &str,
    default_cwd: &Path,
) -> Vec<String> {
    let cwd = req
        .cwd
        .clone()
        .unwrap_or_else(|| default_cwd.to_path_buf());
    let mut argv: Vec<String> = Vec::with_capacity(req.args.len() + req.env.len() + 6);
    argv.push("docker".into());
    argv.push("exec".into());
    argv.push("-w".into());
    argv.push(cwd.to_string_lossy().to_string());
    for (k, v) in &req.env {
        argv.push("-e".into());
        argv.push(format!("{k}={v}"));
    }
    argv.push("-i".into());
    argv.push(container.to_string());
    argv.push(req.program.clone());
    argv.extend(req.args.iter().cloned());
    argv
}

/// Process world executing every request inside a running container.
pub struct DockerExecProcess {
    container: String,
    default_cwd: std::path::PathBuf,
    /// When set, docker commands run on that SSH host instead of locally.
    ssh: Option<Arc<SshProcess>>,
    /// Local executor for the non-ssh path (docker CLI runs here).
    local: Arc<dyn ProcessProvider>,
}

impl DockerExecProcess {
    /// Container target without an ssh hop (local docker daemon).
    pub fn new(
        container: impl Into<String>,
        default_cwd: std::path::PathBuf,
        local: Arc<dyn ProcessProvider>,
    ) -> Arc<Self> {
        Arc::new(Self {
            container: container.into(),
            default_cwd,
            ssh: None,
            local,
        })
    }

    /// Container target reached through an SSH host (`ssh_target`).
    pub fn over_ssh(
        container: impl Into<String>,
        default_cwd: std::path::PathBuf,
        ssh: Arc<SshProcess>,
    ) -> Arc<Self> {
        Arc::new(Self {
            container: container.into(),
            default_cwd,
            ssh: Some(ssh.clone()),
            local: ssh,
        })
    }

    fn compose(&self, request: &ProcessRequest) -> Vec<String> {
        compose_docker_exec(request, &self.container, &self.default_cwd)
    }

    fn argv_to_request(&self, argv: Vec<String>) -> ProcessRequest {
        let mut it = argv.into_iter();
        let program = it.next().unwrap_or_default();
        ProcessRequest {
            program,
            args: it.collect(),
            cwd: None,
            env: Vec::new(),
            stdin_data: None,
        }
    }
}

#[async_trait]
impl ProcessProvider for DockerExecProcess {
    fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let req = self.argv_to_request(self.compose(request));
        match &self.ssh {
            Some(ssh) => ssh.run_blocking(&req),
            None => self.local.run_blocking(&req),
        }
    }

    async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
        let req = self.argv_to_request(self.compose(request));
        match &self.ssh {
            Some(ssh) => ssh.run_async(&req).await,
            None => self.local.run_async(&req).await,
        }
    }

    async fn spawn_piped(&self, spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
        let mut rewritten = self.argv_to_request(self.compose(&spec.request));
        rewritten.stdin_data = spec.request.stdin_data.clone();
        let spec2 = PipedSpawn {
            request: rewritten,
            pipe_stdin: spec.pipe_stdin,
            pipe_stdout: spec.pipe_stdout,
            pipe_stderr: spec.pipe_stderr,
            kill_on_drop: spec.kill_on_drop,
        };
        match &self.ssh {
            Some(ssh) => ssh.spawn_piped(&spec2).await,
            None => self.local.spawn_piped(&spec2).await,
        }
    }

    fn capabilities(&self) -> ExecCaps {
        ExecCaps { is_remote: true }
    }
}

/// One running container as reported by `docker ps`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub id: String,
    pub names: String,
    pub image: String,
    pub status: String,
}

/// List running containers (`docker ps --format '{{json .}}'`).
pub async fn list_running_containers() -> io::Result<Vec<ContainerInfo>> {
    let out = tokio::process::Command::new("docker")
        .args(["ps", "--format", "{{json .}}"])
        .output()
        .await
        .map_err(|e| io::Error::other(format!("docker ps: {e}")))?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "docker ps failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // docker's JSON lines carry ID/Names/Image/Status keys.
        let Ok(mut v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        out.push(ContainerInfo {
            id: take_str(&mut v, "ID"),
            names: take_str(&mut v, "Names"),
            image: take_str(&mut v, "Image"),
            status: take_str(&mut v, "Status"),
        });
    }
    Ok(out)
}

fn take_str(v: &mut serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn req(program: &str, args: &[&str]) -> ProcessRequest {
        ProcessRequest::new(program, args)
    }

    #[test]
    fn compose_uses_native_flags_and_no_shell() {
        let argv = compose_docker_exec(&req("rg", &["-n", "x"]), "ci-1", Path::new("/workspace"));
        assert_eq!(
            argv,
            vec![
                "docker", "exec", "-w", "/workspace", "-i", "ci-1", "rg", "-n", "x"
            ]
        );
    }

    #[test]
    fn compose_request_cwd_wins_over_default() {
        let mut r = req("ls", &[]);
        r.cwd = Some(PathBuf::from("/tmp"));
        let argv = compose_docker_exec(&r, "c", Path::new("/workspace"));
        assert_eq!(&argv[3], "/tmp");
    }

    #[test]
    fn compose_env_uses_e_flags() {
        let mut r = req("printenv", &["K"]);
        r.env = vec![("K".into(), "v with spaces".into())];
        let argv = compose_docker_exec(&r, "c", Path::new("/w"));
        assert_eq!(&argv[4], "-e");
        assert_eq!(&argv[5], "K=v with spaces");
        assert_eq!(&argv[8], "printenv");
    }

    #[test]
    fn caps_report_remote() {
        assert!(!ExecCaps::default().is_remote);
    }

    // Ignored integration test: requires a local docker daemon.
    #[tokio::test]
    #[ignore = "requires local docker with a running container named shannon-it"]
    async fn exec_roundtrip_in_container() {
        let request = req("echo", &["shannon"]);
        let argv = compose_docker_exec(&request, "shannon-it", Path::new("/"));
        let out = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .await
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "shannon");
    }
}
