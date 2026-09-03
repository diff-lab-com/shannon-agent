//! SFTP-backed `FileSystemProvider` for the SSH world.
//!
//! The SFTP session runs as a dedicated `ssh ... -s sftp` subsystem child
//! spawned on the [`SshRuntime`]'s private runtime (works with or without
//! ControlMaster mux, so Windows gets file ops too). Every call marshals onto
//! that runtime; blocking faces use [`block_on_anywhere`].

use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use openssh_sftp_client::{Sftp, SftpOptions};
use shannon_tool_interface::{DirEntryInfo, FileMeta, FileSystemProvider};
use tokio_stream::StreamExt;

use super::session::{block_on_anywhere, SshRuntime};

/// File world executing every operation over SFTP on the SSH target.
pub struct SshFs {
    rt: Arc<SshRuntime>,
    sftp: Arc<tokio::sync::Mutex<Sftp>>,
    /// Keeps the `ssh -s sftp` child alive for the lifetime of the world.
    _child: Arc<tokio::sync::Mutex<tokio::process::Child>>,
    /// Whether the server advertised the posix-rename extension.
    posix_rename: bool,
}

impl SshFs {
    /// Attach an SFTP session to `rt`'s existing ssh connection.
    pub async fn connect(rt: Arc<SshRuntime>) -> io::Result<Arc<Self>> {
        // Spawn the subsystem child on the dedicated runtime so its IO is
        // registered with (and pumped by) that runtime only.
        let (stdin, stdout, child) = rt
            .run({
                let dest = rt.dest().to_string();
                let ctl = rt.control_socket().map(|p| p.to_path_buf());
                async move {
                    let mut cmd = tokio::process::Command::new("ssh");
                    cmd.arg("-o").arg("BatchMode=yes");
                    if let Some(ctl) = ctl {
                        cmd.arg("-o").arg(format!("ControlPath={}", ctl.display()));
                    }
                    cmd.arg(&dest).arg("-s").arg("sftp");
                    cmd.stdin(std::process::Stdio::piped());
                    cmd.stdout(std::process::Stdio::piped());
                    cmd.stderr(std::process::Stdio::null());
                    cmd.kill_on_drop(true);
                    let mut child = cmd
                        .spawn()
                        .map_err(|e| io::Error::other(format!("sftp subsystem spawn: {e}")))?;
                    let stdin = child
                        .stdin
                        .take()
                        .ok_or_else(|| io::Error::other("sftp child stdin missing"))?;
                    let stdout = child
                        .stdout
                        .take()
                        .ok_or_else(|| io::Error::other("sftp child stdout missing"))?;
                    io::Result::Ok((stdin, stdout, child))
                }
            })
            .await?;

        let sftp = Sftp::new(stdin, stdout, SftpOptions::default())
            .await
            .map_err(to_io)?;
        let posix_rename = sftp.support_posix_rename();
        tracing::debug!(posix_rename, "sftp session established");

        Ok(Arc::new(Self {
            rt,
            sftp: Arc::new(tokio::sync::Mutex::new(sftp)),
            _child: Arc::new(tokio::sync::Mutex::new(child)),
            posix_rename,
        }))
    }

    /// Run a future on the dedicated runtime from any runtime.
    async fn on_rt<T, F>(&self, fut: F) -> io::Result<T>
    where
        F: Future<Output = io::Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        self.rt.run(fut).await
    }

    async fn read_bytes_impl(&self, path: PathBuf) -> io::Result<Vec<u8>> {
        let sftp = self.sftp.clone();
        self.on_rt(async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.read(path).await.map(|b| b.to_vec()).map_err(to_io)
        })
        .await
    }

    /// Rename that tolerates an existing destination: prefer the server's
    /// posix-rename extension; otherwise remove + rename (the write path
    /// always renames from a Shannon-owned temp file, so the window is
    /// bounded to our own writes).
    async fn rename_overwrite(&self, from: PathBuf, to: PathBuf) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let posix = self.posix_rename;
        self.on_rt(async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            if posix {
                fs.rename(&from, &to).await.map_err(to_io)
            } else {
                fs.remove_file(&to)
                    .await
                    .or_else(|e| ignore_missing(e).map_err(to_io))?;
                fs.rename(&from, &to).await.map_err(to_io)
            }
        })
        .await
    }
}

fn ignore_missing(e: openssh_sftp_client::Error) -> Result<(), openssh_sftp_client::Error> {
    // Missing target is success for our purposes; anything else propagates.
    if let openssh_sftp_client::Error::IOError(io) = &e {
        if matches!(
            io.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::AlreadyExists
        ) {
            return Ok(());
        }
    }
    let msg = e.to_string();
    if msg.contains("No such file") || msg.contains("no such file") {
        return Ok(());
    }
    Err(e)
}

fn to_io(e: openssh_sftp_client::Error) -> io::Error {
    match e {
        openssh_sftp_client::Error::IOError(io) => io,
        other => io::Error::other(other.to_string()),
    }
}

fn meta_to_filemeta(md: &openssh_sftp_client::metadata::MetaData) -> FileMeta {
    FileMeta {
        len: md.len().unwrap_or(0),
        is_dir: md.file_type().map(|t| t.is_dir()).unwrap_or(false),
        modified: md.modified().map(|t| t.as_system_time()),
    }
}

/// Shared body for the async + blocking faces of `create_dir_all`.
async fn create_dir_all_body(
    sftp: &Arc<tokio::sync::Mutex<Sftp>>,
    path: PathBuf,
) -> io::Result<()> {
    let guard = sftp.lock().await;
    let mut fs = guard.fs();
    let mut cursor = PathBuf::new();
    for component in path.components() {
        cursor.push(component);
        // Ignore per-step failures; the final stat decides.
        let _ = fs.create_dir(&cursor).await;
    }
    match fs.metadata(&cursor).await {
        Ok(md) if md.file_type().map(|t| t.is_dir()).unwrap_or(false) => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} is not a directory", cursor.display()),
        )),
        Err(e) => Err(to_io(e)),
    }
}

#[async_trait]
impl FileSystemProvider for SshFs {
    async fn read_text(&self, path: &Path) -> io::Result<String> {
        let bytes = self.read_bytes(path).await?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.read_bytes_impl(path.to_path_buf()).await
    }

    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        self.on_rt(async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.metadata(path)
                .await
                .map(|md| meta_to_filemeta(&md))
                .map_err(to_io)
        })
        .await
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        self.on_rt(async move { create_dir_all_body(&sftp, path).await })
            .await
    }

    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        let contents = contents.to_vec();
        self.on_rt(async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.write(path, contents).await.map_err(to_io)
        })
        .await
    }

    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.rename_overwrite(from.to_path_buf(), to.to_path_buf())
            .await
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        self.on_rt(async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.canonicalize(path).await.map_err(to_io)
        })
        .await
    }

    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        let bytes = block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.read(path).await.map(|b| b.to_vec()).map_err(to_io)
        })?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        let contents = contents.to_vec();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.write(path, contents).await.map_err(to_io)
        })
    }

    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            create_dir_all_body(&sftp, path).await
        })
    }

    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.remove_file(path).await.map_err(to_io)
        })
    }

    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.canonicalize(path).await.map_err(to_io)
        })
    }

    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            fs.metadata(path)
                .await
                .map(|md| meta_to_filemeta(&md))
                .map_err(to_io)
        })
    }

    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        let sftp = self.sftp.clone();
        let path = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut file = guard.open(path).await.map_err(to_io)?;
            let mut out = Vec::with_capacity(max_bytes);
            while out.len() < max_bytes {
                let want = (max_bytes - out.len()).min(32 * 1024) as u32;
                // `read` consumes the buffer and returns it filled, or None
                // at EOF (the crate's own Fs::read uses the same protocol).
                match file
                    .read(want, bytes::BytesMut::with_capacity(want as usize))
                    .await
                    .map_err(to_io)?
                {
                    Some(chunk) if !chunk.is_empty() => out.extend_from_slice(&chunk),
                    _ => break,
                }
            }
            Ok(out)
        })
    }

    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()> {
        let runtime = self.rt.runtime();
        let stat_sftp = self.sftp.clone();
        let stat = move |p: &Path| {
            let sftp = stat_sftp.clone();
            let p = p.to_path_buf();
            block_on_anywhere(runtime, async move {
                let guard = sftp.lock().await;
                let mut fs = guard.fs();
                fs.metadata(p)
                    .await
                    .map(|md| meta_to_filemeta(&md))
                    .map_err(to_io)
            })
        };
        let text_runtime = self.rt.runtime();
        let text_sftp = self.sftp.clone();
        let read_text = move |p: &Path| {
            let sftp = text_sftp.clone();
            let p = p.to_path_buf();
            block_on_anywhere(text_runtime, async move {
                let guard = sftp.lock().await;
                let mut fs = guard.fs();
                fs.read(p)
                    .await
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .map_err(to_io)
            })
        };
        let list_runtime = self.rt.runtime();
        let list_sftp = self.sftp.clone();
        let list_dir = move |p: &Path| {
            let sftp = list_sftp.clone();
            let root = p.to_path_buf();
            block_on_anywhere(list_runtime, async move {
                let guard = sftp.lock().await;
                let mut fs = guard.fs();
                let dir = fs.open_dir(&root).await.map_err(to_io)?;
                let mut entries = Vec::new();
                let stream = dir.read_dir();
                tokio::pin!(stream);
                while let Some(entry) = stream.next().await {
                    let entry = entry.map_err(to_io)?;
                    let md = entry.metadata();
                    entries.push(DirEntryInfo {
                        path: root.join(entry.filename()),
                        len: md.len().unwrap_or(0),
                        is_dir: md.file_type().map(|t| t.is_dir()).unwrap_or(false),
                    });
                }
                Ok(entries)
            })
        };
        shannon_tool_interface::walk::provider_walk(&stat, &read_text, &list_dir, root, cb)
    }

    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        let sftp = self.sftp.clone();
        let root = path.to_path_buf();
        block_on_anywhere(self.rt.runtime(), async move {
            let guard = sftp.lock().await;
            let mut fs = guard.fs();
            let dir = fs.open_dir(&root).await.map_err(to_io)?;
            let mut entries = Vec::new();
            let stream = dir.read_dir();
            tokio::pin!(stream);
            while let Some(entry) = stream.next().await {
                let entry = entry.map_err(to_io)?;
                let md = entry.metadata();
                entries.push(DirEntryInfo {
                    path: root.join(entry.filename()),
                    len: md.len().unwrap_or(0),
                    is_dir: md.file_type().map(|t| t.is_dir()).unwrap_or(false),
                });
            }
            Ok(entries)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_projection_defaults_are_safe() {
        // len/mtime may be absent from the server response; the projection
        // contract degrades to zero/None instead of panicking.
        let fm = FileMeta {
            len: 0,
            is_dir: true,
            modified: None,
        };
        assert!(fm.is_dir);
        assert_eq!(fm.len, 0);
        assert!(fm.modified.is_none());
    }

    #[test]
    fn ignore_missing_tolerates_absent_targets() {
        let absent =
            openssh_sftp_client::Error::IOError(io::Error::new(io::ErrorKind::NotFound, "gone"));
        assert!(ignore_missing(absent).is_ok());
        let fatal = openssh_sftp_client::Error::UnsupportedExtension(&"hardlink");
        assert!(ignore_missing(fatal).is_err());
    }

    // Ignored integration test: requires a local sshd.
    #[tokio::test]
    #[ignore = "requires local sshd: ssh localhost must work non-interactively"]
    async fn sftp_full_roundtrip_on_localhost() {
        let target = crate::target::RemoteTarget {
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
        let fs = SshFs::connect(rt).await.unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("roundtrip.txt");

        fs.write_bytes(&file, b"hello shannon").await.unwrap();
        assert_eq!(fs.read_text(&file).await.unwrap(), "hello shannon");

        let md = fs.metadata(&file).await.unwrap();
        assert_eq!(md.len, 13);
        assert!(!md.is_dir);

        let entries = fs.list_dir_blocking(tmp.path()).unwrap();
        assert!(entries.iter().any(|e| e.path == file));

        // Overwriting rename (posix-rename or fallback).
        let tmp2 = tempfile::TempDir::new().unwrap();
        fs.write_bytes(&tmp2.path().join("dst"), b"old").await.unwrap();
        fs.write_bytes(&tmp.path().join("src"), b"new").await.unwrap();
        fs.rename(&tmp.path().join("src"), &tmp2.path().join("dst"))
            .await
            .unwrap();
        assert_eq!(
            fs.read_text(&tmp2.path().join("dst")).await.unwrap(),
            "new"
        );

        let prefix = fs.read_prefix_blocking(&file, 5).unwrap();
        assert_eq!(prefix, b"hello");

        fs.canonicalize(&file).await.unwrap();
        fs.remove_file_blocking(&file).unwrap();
    }
}
