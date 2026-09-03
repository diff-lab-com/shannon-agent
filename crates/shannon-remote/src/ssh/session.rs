//! SshRuntime: one dedicated tokio runtime per SSH target.
//!
//! openssh `Session`/`Child` IO objects are bound to the runtime that created
//! them. Every session object is therefore created and driven on a private
//! runtime (own background thread), and the public methods marshal calls onto
//! it — `JoinHandle` await is runtime-independent, so callers on the
//! application runtime can simply `.await`.

use std::future::Future;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use shannon_tool_interface::{CapturedOutput, PipedChild, ProcessExit};
use tokio::io::AsyncWriteExt;
use tokio::runtime::{Handle, Runtime};

use crate::target::RemoteTarget;

/// Liveness of the SSH transport behind a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorldStatus {
    /// No remote target (local world).
    #[default]
    Local,
    /// Session established; commands are being served.
    Connected,
    /// Last transport attempt failed; the world is degraded and callers
    /// should offer an explicit reconnect.
    Degraded,
}

/// Facts gathered about a target during health check (`/remote use`, Test).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// `uname -s` output (e.g. `Linux`), trimmed.
    pub platform: String,
    /// Remote `$HOME`.
    pub home: String,
    /// Whether `bash` exists on the target (the bash tool requires it).
    pub bash_available: bool,
    /// Whether the configured workspace_dir exists.
    pub workspace_exists: bool,
    /// Round-trip time of the combined probe command.
    pub latency_ms: u64,
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[allow(dead_code)] // documentation value for the 0 state
const STATUS_LOCAL: u8 = 0;
const STATUS_CONNECTED: u8 = 1;
const STATUS_DEGRADED: u8 = 2;

type ArcChild = openssh::Child<Arc<openssh::Session>>;

/// Owns the SSH session for one target plus the runtime that drives it.
pub struct SshRuntime {
    dest: String,
    workspace_dir: std::path::PathBuf,
    rt: Runtime,
    session: Arc<openssh::Session>,
    status: AtomicU8,
}

impl std::fmt::Debug for SshRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshRuntime")
            .field("dest", &self.dest)
            .field("workspace_dir", &self.workspace_dir)
            .finish_non_exhaustive()
    }
}

impl SshRuntime {
    /// Connect to `target`'s host. BatchMode is always on (openssh enforces
    /// it), so a missing agent/key fails fast instead of hanging on a
    /// passphrase prompt; `StrictHostKeyChecking=accept-new` gives TOFU via
    /// the system known_hosts.
    pub async fn connect(target: &RemoteTarget) -> io::Result<Arc<Self>> {
        let dest = target.ssh_destination();
        let workspace_dir = target.workspace_dir.clone();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| io::Error::other(format!("ssh runtime: {e}")))?;
        let session = spawn_on(&rt, {
            let dest = dest.clone();
            async move {
                let mut builder = openssh::SessionBuilder::default();
                builder
                    .known_hosts_check(openssh::KnownHosts::Add)
                    .connect_timeout(CONNECT_TIMEOUT);
                connect_with(&builder, &dest)
                    .await
                    .map_err(|e| io::Error::other(e.to_string()))
            }
        })
        .await?;

        Ok(Arc::new(Self {
            dest,
            workspace_dir,
            rt,
            session: Arc::new(session),
            status: AtomicU8::new(STATUS_CONNECTED),
        }))
    }

    /// The ssh destination this runtime is bound to.
    pub fn dest(&self) -> &str {
        &self.dest
    }

    /// Control socket of the ssh ControlMaster (Unix mux only).
    pub(crate) fn control_socket(&self) -> Option<&Path> {
        #[cfg(unix)]
        {
            Some(self.session.control_socket())
        }
        #[cfg(windows)]
        {
            None
        }
    }

    /// The private runtime driving this session (borrowed; do not block).
    pub(crate) fn runtime(&self) -> &Runtime {
        &self.rt
    }

    /// Marshal a future onto the dedicated runtime; awaitable from any
    /// runtime.
    pub(crate) async fn run<T, F>(&self, fut: F) -> io::Result<T>
    where
        F: Future<Output = io::Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        match self.rt.spawn(fut).await {
            Ok(v) => v,
            Err(e) => Err(io::Error::other(format!("ssh runtime join: {e}"))),
        }
    }

    /// Current transport status.
    pub fn status(&self) -> WorldStatus {
        match self.status.load(Ordering::Relaxed) {
            STATUS_CONNECTED => WorldStatus::Connected,
            STATUS_DEGRADED => WorldStatus::Degraded,
            _ => WorldStatus::Local,
        }
    }

    fn set_status(&self, s: u8) {
        self.status.store(s, Ordering::Relaxed);
    }

    /// Run one captured command (`argv` is shell-quoted per element by
    /// openssh). Exit code 255 marks a transport failure and degrades the
    /// runtime; a later success restores `Connected`.
    pub async fn exec(self: &Arc<Self>, argv: Vec<String>) -> io::Result<CapturedOutput> {
        let started = Instant::now();
        let out = spawn_on(&self.rt, {
            let session = self.session.clone();
            let argv = argv.clone();
            async move {
                let Some((program, args)) = argv.split_first() else {
                    return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty argv"));
                };
                let mut cmd = session.arc_command(program.clone());
                cmd.args(args.iter().cloned());
                cmd.output()
                    .await
                    .map_err(|e| io::Error::other(e.to_string()))
            }
        })
        .await?;

        let captured = CapturedOutput {
            stdout: out.stdout,
            stderr: out.stderr,
            exit: ProcessExit {
                code: out.status.code(),
                success: out.status.success(),
            },
        };
        if captured.exit.code == Some(255) {
            self.set_status(STATUS_DEGRADED);
        } else if self.status() == WorldStatus::Degraded && captured.exit.success {
            // A successful round-trip proves the transport is back.
            self.set_status(STATUS_CONNECTED);
        }
        tracing::trace!(dest = %self.dest, ms = started.elapsed().as_millis() as u64, "ssh exec");
        Ok(captured)
    }

    /// Blocking capture bridge for sync call sites (git helpers). Marshals
    /// onto the dedicated runtime via [`block_on_anywhere`].
    pub fn exec_blocking(self: &Arc<Self>, argv: Vec<String>) -> io::Result<CapturedOutput> {
        block_on_anywhere(&self.rt, {
            let this = self.clone();
            async move { this.exec(argv).await }
        })
    }

    /// Probe platform/home/bash/workspace in one round trip.
    pub async fn health(self: &Arc<Self>) -> io::Result<HealthReport> {
        let ws = self.workspace_dir.to_string_lossy().to_string();
        let started = Instant::now();
        let out = self
            .exec(vec![
                "sh".into(),
                "-c".into(),
                // NUL-separated fields; single fixed literal script, the
                // workspace path rides in as a positional argument.
                "uname -s; printf '%s\\0' \"$HOME\"; command -v bash >/dev/null 2>&1 && printf b1 || printf b0; test -d \"$1\" && printf w1 || printf w0".into(),
                "sh".into(),
                ws,
            ])
            .await?;
        let latency_ms = started.elapsed().as_millis() as u64;

        if !out.exit.success {
            return Err(io::Error::other(format!(
                "health probe failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        let mut fields = text.split('\0');
        let platform = fields.next().unwrap_or("").trim().to_string();
        let home = fields.next().unwrap_or("").to_string();
        let flags = fields.next().unwrap_or("b0w0").to_string();
        Ok(HealthReport {
            platform,
            home,
            bash_available: flags.contains("b1"),
            workspace_exists: flags.contains("w1"),
            latency_ms,
        })
    }

    /// Spawn `argv` on the target with piped streams and hand back a
    /// [`PipedChild`] whose streams are runtime-agnostic duplex halves.
    pub(crate) async fn spawn_piped_argv(
        self: &Arc<Self>,
        argv: Vec<String>,
        pipe_stdin: bool,
        pipe_stdout: bool,
        pipe_stderr: bool,
    ) -> io::Result<Box<dyn PipedChild>> {
        let (child, stdin, stdout, stderr) = spawn_on(&self.rt, {
            let session = self.session.clone();
            let argv = argv.clone();
            async move {
                let (program, args) = argv
                    .split_first()
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty argv"))?;
                let mut cmd = session.arc_command(program.clone());
                cmd.args(args.iter().cloned());
                cmd.stdin(if pipe_stdin {
                    openssh::Stdio::piped()
                } else {
                    openssh::Stdio::null()
                });
                cmd.stdout(if pipe_stdout {
                    openssh::Stdio::piped()
                } else {
                    openssh::Stdio::null()
                });
                cmd.stderr(if pipe_stderr {
                    openssh::Stdio::piped()
                } else {
                    openssh::Stdio::null()
                });
                let mut child = cmd
                    .spawn()
                    .await
                    .map_err(|e| io::Error::other(format!("ssh spawn: {e}")))?;
                let stdin = child.stdin().take();
                let stdout = child.stdout().take();
                let stderr = child.stderr().take();
                io::Result::Ok((child, stdin, stdout, stderr))
            }
        })
        .await?;

        let child_slot = Arc::new(tokio::sync::Mutex::new(Some(child)));
        let (mut client_in, remote_in) = tokio::io::duplex(64 * 1024);
        let (mut remote_out, client_out) = tokio::io::duplex(64 * 1024);
        let (mut remote_err, client_err) = tokio::io::duplex(64 * 1024);
        let owner = self.rt.handle().clone();

        // stdin pump: caller writes duplex half -> ssh child stdin.
        if let Some(mut sink) = stdin {
            owner.spawn(async move {
                let _ = tokio::io::copy(&mut client_in, &mut sink).await;
                let _ = sink.shutdown().await;
            });
        } else {
            client_in.shutdown().await.ok();
        }
        // stdout pump: ssh child stdout -> caller reads duplex half.
        if let Some(mut source) = stdout {
            owner.spawn(async move {
                let _ = tokio::io::copy(&mut source, &mut remote_out).await;
                let _ = remote_out.shutdown().await;
            });
        } else {
            remote_out.shutdown().await.ok();
        }
        // stderr pump: ssh child stderr -> caller reads duplex half.
        if let Some(mut source) = stderr {
            owner.spawn(async move {
                let _ = tokio::io::copy(&mut source, &mut remote_err).await;
                let _ = remote_err.shutdown().await;
            });
        } else {
            remote_err.shutdown().await.ok();
        }

        Ok(Box::new(SshPipedChild {
            owner,
            child_slot,
            stdin: remote_in,
            stdout: client_out,
            stderr: client_err,
        }))
    }
}

/// [`PipedChild`] adapter over an openssh remote child.
///
/// Streams are `tokio::io::duplex` halves — runtime-agnostic in-memory pipes
/// — while the actual ssh channel is pumped on the owning runtime, so callers
/// may poll from the application runtime without crossing runtime boundaries.
struct SshPipedChild {
    owner: Handle,
    child_slot: Arc<tokio::sync::Mutex<Option<ArcChild>>>,
    stdin: tokio::io::DuplexStream,
    stdout: tokio::io::DuplexStream,
    stderr: tokio::io::DuplexStream,
}

#[async_trait::async_trait]
impl PipedChild for SshPipedChild {
    fn take_stdin(&mut self) -> Option<Box<dyn tokio::io::AsyncWrite + Send + Unpin>> {
        Some(Box::new(std::mem::replace(
            &mut self.stdin,
            tokio::io::duplex(1).0,
        )))
    }

    fn take_stdout(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        Some(Box::new(std::mem::replace(
            &mut self.stdout,
            tokio::io::duplex(1).1,
        )))
    }

    fn take_stderr(&mut self) -> Option<Box<dyn tokio::io::AsyncRead + Send + Unpin>> {
        Some(Box::new(std::mem::replace(
            &mut self.stderr,
            tokio::io::duplex(1).1,
        )))
    }

    async fn kill(&mut self) {
        // openssh sets kill_on_drop on the underlying local ssh client;
        // dropping tears the channel down (the remote process gets HUP when
        // the mux channel closes).
        self.child_slot.lock().await.take();
    }

    async fn wait(&mut self) -> io::Result<ProcessExit> {
        let child = self.child_slot.lock().await.take();
        let Some(child) = child else {
            return Err(io::Error::other("ssh child already reaped or killed"));
        };
        let status = self
            .owner
            .spawn(async move { child.wait().await })
            .await
            .map_err(|e| io::Error::other(format!("ssh wait join: {e}")))?
            .map_err(|e| io::Error::other(format!("ssh wait: {e}")))?;
        Ok(ProcessExit {
            code: status.code(),
            success: status.success(),
        })
    }
}

/// Run `fut` on runtime `rt` and await the result from any runtime.
async fn spawn_on<T, F>(rt: &Runtime, fut: F) -> io::Result<T>
where
    F: Future<Output = io::Result<T>> + Send + 'static,
    T: Send + 'static,
{
    match rt.spawn(fut).await {
        Ok(v) => v,
        Err(e) => Err(io::Error::other(format!("ssh runtime join: {e}"))),
    }
}

/// `block_on` from any thread, including threads already inside another
/// tokio runtime (spawn_blocking workers): hop to a plain OS thread when
/// necessary. Blocking ssh helpers call this.
pub(crate) fn block_on_anywhere<T, F>(rt: &Runtime, fut: F) -> T
where
    F: Future<Output = T> + Send,
    T: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|s| s.spawn(|| rt.block_on(fut)).join().expect("block_on thread"))
    } else {
        rt.block_on(fut)
    }
}

/// Connect with the default process-mux session: ControlMaster multiplexing
/// on Unix, per-command ssh processes on Windows (no ControlMaster there).
async fn connect_with(
    builder: &openssh::SessionBuilder,
    dest: &str,
) -> Result<openssh::Session, openssh::Error> {
    builder.connect(dest).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_mapping_covers_all_variants() {
        let map = |v: u8| match v {
            STATUS_CONNECTED => WorldStatus::Connected,
            STATUS_DEGRADED => WorldStatus::Degraded,
            _ => WorldStatus::Local,
        };
        assert_eq!(map(STATUS_LOCAL), WorldStatus::Local);
        assert_eq!(map(STATUS_CONNECTED), WorldStatus::Connected);
        assert_eq!(map(STATUS_DEGRADED), WorldStatus::Degraded);
    }

    #[test]
    fn health_parses_probe_fields() {
        let text = "Linux\0/home/ed\0b1w1\0rest";
        let mut fields = text.split('\0');
        let platform = fields.next().unwrap_or("").trim().to_string();
        let home = fields.next().unwrap_or("").to_string();
        let flags = fields.next().unwrap_or("b0w0").to_string();
        assert_eq!(platform, "Linux");
        assert_eq!(home, "/home/ed");
        assert!(flags.contains("b1") && flags.contains("w1"));

        // Missing flags degrade to false, never panic.
        let flags = fields.next().unwrap_or("b0w0").to_string();
        assert!(!flags.contains("b1"));
    }

    #[test]
    fn block_on_anywhere_works_outside_and_inside_runtimes() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // Outside any runtime.
        assert_eq!(block_on_anywhere(&rt, async { 41 + 1 }), 42);
        // Inside another runtime (tokio::test worker).
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                assert_eq!(block_on_anywhere(&rt, async { 6 * 7 }), 42);
            });
    }

    // Ignored integration test: requires a local sshd reachable as `localhost`.
    #[tokio::test]
    #[ignore = "requires local sshd: ssh localhost must work non-interactively"]
    async fn exec_roundtrip_on_localhost() {
        let target = RemoteTarget {
            name: "it".into(),
            kind: crate::target::TargetKind::Ssh,
            host: Some("localhost".into()),
            port: None,
            user: None,
            container: None,
            shell: None,
            ssh_target: None,
            workspace_dir: std::env::temp_dir(),
        };
        let rt = SshRuntime::connect(&target).await.unwrap();
        let health = rt.health().await.unwrap();
        assert!(!health.platform.is_empty());
        assert!(health.home.starts_with('/'));
        let out = rt.exec(vec!["echo".into(), "shannon".into()]).await.unwrap();
        assert!(out.exit.success);
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "shannon");

        // spawn_piped round-trip through the duplex bridge.
        let mut child = rt
            .spawn_piped_argv(vec!["cat".into()], true, true, false)
            .await
            .unwrap();
        let mut stdin = child.take_stdin().unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stdin.write_all(b"ping").await.unwrap();
        stdin.shutdown().await.unwrap();
        let mut stdout = child.take_stdout().unwrap();
        let mut buf = Vec::new();
        stdout.read_to_end(&mut buf).await.unwrap();
        assert_eq!(buf, b"ping");
    }
}
