//! `docker exec`-backed `FileSystemProvider`.
//!
//! Containers rarely ship an SFTP server, so file operations are composed
//! from POSIX tools inside the container:
//! - read/write: `cat` / `cat > path` (binary-safe via stdin/stdout)
//! - list: `find <dir> -mindepth 1 -maxdepth 1 -type d/-type f -print0`
//!   (`-mindepth 1` is load-bearing — without it a directory lists itself
//!   and the provider walk would recurse forever; `-print0` survives
//!   newlines and locales in filenames)
//! - metadata: `stat -c '%s %Y'` (busybox-compatible; seconds precision)
//! - rename: `mv -f`; canonicalize: `readlink -f`; mkdir: `mkdir -p`

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use shannon_tool_interface::{
    DirEntryInfo, FileMeta, FileSystemProvider, ProcessProvider, ProcessRequest,
};

use crate::docker::process::DockerExecProcess;

/// File world executing every operation inside a running container.
pub struct DockerExecFs {
    proc: Arc<DockerExecProcess>,
}

impl DockerExecFs {
    /// File world paired with its container process world (shares the
    /// optional ssh hop and container name).
    pub fn new(proc: Arc<DockerExecProcess>) -> Arc<Self> {
        Arc::new(Self { proc })
    }
}

/// Build a `program args...` request routed through the container world.
fn container_request(program: &str, args: Vec<String>) -> ProcessRequest {
    let mut r = ProcessRequest::new(program, &[]);
    r.args = args;
    r
}

/// Run one composed container command to completion through the world.
async fn run(proc: &DockerExecProcess, program: &str, args: &[String]) -> io::Result<Captured> {
    let out = proc
        .run_async(&container_request(program, args.to_vec()))
        .await?;
    Ok(Captured {
        stdout: out.stdout,
        stderr: out.stderr,
        code: out.exit.code,
    })
}

/// Blocking `stat -c` metadata probe.
fn metadata_sync(proc: &DockerExecProcess, path: &Path) -> io::Result<FileMeta> {
    let cap = run_blocking(
        proc,
        "stat",
        &[
            "-c".to_string(),
            "%s %Y %F".to_string(),
            path.to_string_lossy().to_string(),
        ],
    )?;
    cap.ok("stat")?;
    parse_stat(&String::from_utf8_lossy(&cap.stdout))
        .ok_or_else(|| io::Error::other(format!("stat parse failed for {}", path.display())))
}

/// Blocking `-print0` find-based listing.
fn list_dir_sync(proc: &DockerExecProcess, root: &Path) -> io::Result<Vec<DirEntryInfo>> {
    let root_arg = root.to_string_lossy().to_string();
    let dirs = run_blocking(
        proc,
        "find",
        &[
            root_arg.clone(),
            "-mindepth".to_string(),
            "1".to_string(),
            "-maxdepth".to_string(),
            "1".to_string(),
            "-type".to_string(),
            "d".to_string(),
            "-print0".to_string(),
        ],
    )?;
    dirs.ok("find -type d")?;
    let files = run_blocking(
        proc,
        "find",
        &[
            root_arg,
            "-mindepth".to_string(),
            "1".to_string(),
            "-maxdepth".to_string(),
            "1".to_string(),
            "-type".to_string(),
            "f".to_string(),
            "-print0".to_string(),
        ],
    )?;
    files.ok("find -type f")?;
    Ok(merge_find_entries(root, &dirs.stdout, &files.stdout))
}

/// Blocking twin of [`run`].
fn run_blocking(proc: &DockerExecProcess, program: &str, args: &[String]) -> io::Result<Captured> {
    let out = proc.run_blocking(&container_request(program, args.to_vec()))?;
    Ok(Captured {
        stdout: out.stdout,
        stderr: out.stderr,
        code: out.exit.code,
    })
}

struct Captured {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
}

impl Captured {
    fn ok(&self, op: &str) -> io::Result<()> {
        if self.code == Some(0) {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "docker {op} failed ({}): {}",
                self.code.unwrap_or(-1),
                String::from_utf8_lossy(&self.stderr).trim()
            )))
        }
    }
}

fn path_arg(op: &str, path: &Path) -> String {
    let _ = op;
    path.to_string_lossy().to_string()
}

#[async_trait]
impl FileSystemProvider for DockerExecFs {
    async fn read_text(&self, path: &Path) -> io::Result<String> {
        let bytes = self.read_bytes(path).await?;
        String::from_utf8(bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_bytes(&self, path: &Path) -> io::Result<Vec<u8>> {
        let cap = run(&self.proc, "cat", &[path_arg("cat", path)]).await?;
        cap.ok("cat")?;
        Ok(cap.stdout)
    }

    async fn metadata(&self, path: &Path) -> io::Result<FileMeta> {
        let p = path_arg("stat", path);
        let cap = run(&self.proc, "stat", &["-c".into(), "%s %Y %F".into(), p]).await?;
        cap.ok("stat")?;
        parse_stat(&String::from_utf8_lossy(&cap.stdout))
            .ok_or_else(|| io::Error::other(format!("stat parse failed for {}", path.display())))
    }

    async fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        run(&self.proc, "mkdir", &["-p".into(), path_arg("mkdir", path)])
            .await?
            .ok("mkdir -p")
    }

    async fn write_bytes(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        // `cat > "$1"` with the destination as a positional parameter: no
        // interpolation of the path into the script.
        let mut r = ProcessRequest::new("sh", &["-c", "cat > \"$1\"", "sh"]);
        r.args.push(path_arg("cat", path));
        r.stdin_data = Some(contents.to_vec());
        let out = self.proc.run_async(&r).await?;
        if out.exit.success {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "docker write failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    async fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        run(
            &self.proc,
            "mv",
            &["-f".into(), path_arg("mv", from), path_arg("mv", to)],
        )
        .await?
        .ok("mv")
    }

    async fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        let cap = run(
            &self.proc,
            "readlink",
            &["-f".into(), path_arg("readlink", path)],
        )
        .await?;
        cap.ok("readlink -f")?;
        let s = String::from_utf8_lossy(&cap.stdout).trim().to_string();
        if s.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("cannot canonicalize {}", path.display()),
            ))
        } else {
            Ok(PathBuf::from(s))
        }
    }

    fn read_text_blocking(&self, path: &Path) -> io::Result<String> {
        let cap = run_blocking(&self.proc, "cat", &[path_arg("cat", path)])?;
        cap.ok("cat")?;
        String::from_utf8(cap.stdout).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    fn write_bytes_blocking(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        let mut r = ProcessRequest::new("sh", &["-c", "cat > \"$1\"", "sh"]);
        r.args.push(path_arg("cat", path));
        r.stdin_data = Some(contents.to_vec());
        let out = self.proc.run_blocking(&r)?;
        if out.exit.success {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "docker write failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )))
        }
    }

    fn create_dir_all_blocking(&self, path: &Path) -> io::Result<()> {
        run_blocking(&self.proc, "mkdir", &["-p".into(), path_arg("mkdir", path)])?.ok("mkdir -p")
    }

    fn remove_file_blocking(&self, path: &Path) -> io::Result<()> {
        run_blocking(&self.proc, "rm", &["-f".into(), path_arg("rm", path)])?.ok("rm")
    }

    fn canonicalize_blocking(&self, path: &Path) -> io::Result<PathBuf> {
        let cap = run_blocking(
            &self.proc,
            "readlink",
            &["-f".into(), path_arg("readlink", path)],
        )?;
        cap.ok("readlink -f")?;
        let s = String::from_utf8_lossy(&cap.stdout).trim().to_string();
        if s.is_empty() {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("cannot canonicalize {}", path.display()),
            ))
        } else {
            Ok(PathBuf::from(s))
        }
    }

    fn metadata_blocking(&self, path: &Path) -> io::Result<FileMeta> {
        let p = path_arg("stat", path);
        let cap = run_blocking(&self.proc, "stat", &["-c".into(), "%s %Y %F".into(), p])?;
        cap.ok("stat")?;
        parse_stat(&String::from_utf8_lossy(&cap.stdout))
            .ok_or_else(|| io::Error::other(format!("stat parse failed for {}", path.display())))
    }

    fn read_prefix_blocking(&self, path: &Path, max_bytes: usize) -> io::Result<Vec<u8>> {
        // `head -c` keeps the transfer bounded for binary sniffing.
        let cap = run_blocking(
            &self.proc,
            "head",
            &["-c".into(), max_bytes.to_string(), path_arg("head", path)],
        )?;
        cap.ok("head -c")?;
        Ok(cap.stdout)
    }

    fn walk_blocking(
        &self,
        root: &Path,
        cb: &mut dyn FnMut(&DirEntryInfo) -> bool,
    ) -> io::Result<()> {
        let proc = self.proc.clone();
        let stat = move |p: &Path| metadata_sync(&proc, p);
        let proc2 = self.proc.clone();
        let read_text = move |p: &Path| -> io::Result<String> {
            let cap = run_blocking(&proc2, "cat", &[p.to_string_lossy().to_string()])?;
            cap.ok("cat")?;
            String::from_utf8(cap.stdout).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        };
        let proc3 = self.proc.clone();
        let list_dir = move |p: &Path| list_dir_sync(&proc3, p);
        shannon_tool_interface::walk::provider_walk(&stat, &read_text, &list_dir, root, cb)
    }

    fn list_dir_blocking(&self, path: &Path) -> io::Result<Vec<DirEntryInfo>> {
        list_dir_sync(&self.proc, path)
    }
}

/// Parse `stat -c '%s %Y %F'` output: `<len> <mtime> <type words...>`.
fn parse_stat(out: &str) -> Option<FileMeta> {
    let mut parts = out.trim().splitn(3, ' ');
    let len = parts.next()?.parse::<u64>().ok()?;
    let mtime = parts.next()?.parse::<u64>().ok();
    let type_words = parts.next().unwrap_or("");
    let is_dir = type_words.starts_with("directory");
    Some(FileMeta {
        len,
        is_dir,
        modified: mtime
            .map(|s| std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(s)),
    })
}

/// Merge the two `-print0` find outputs into sorted [`DirEntryInfo`]s.
/// Directory entries carry `len = 0` (documented limitation).
fn merge_find_entries(root: &Path, dirs: &[u8], files: &[u8]) -> Vec<DirEntryInfo> {
    let mut out = Vec::new();
    for chunk in dirs.split(|b| *b == 0) {
        if chunk.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(chunk).to_string();
        out.push(DirEntryInfo {
            path: root.join(name),
            len: 0,
            is_dir: true,
        });
    }
    for chunk in files.split(|b| *b == 0) {
        if chunk.is_empty() {
            continue;
        }
        let name = String::from_utf8_lossy(chunk).to_string();
        out.push(DirEntryInfo {
            path: root.join(name),
            len: 0,
            is_dir: false,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parses_gnu_and_busybox_output() {
        let gnu = parse_stat("4096 1720000000 regular file");
        assert_eq!(gnu.unwrap().len, 4096);
        assert!(!gnu.as_ref().unwrap().is_dir);

        let dir = parse_stat("4096 1720000000 directory");
        assert!(dir.unwrap().is_dir);

        let busybox = parse_stat("123 1700000000 regular");
        assert_eq!(busybox.unwrap().len, 123);
        assert!(busybox.as_ref().unwrap().modified.is_some());

        assert!(parse_stat("garbage").is_none());
    }

    #[test]
    fn find_merge_is_sorted_and_nul_safe() {
        let root = Path::new("/w");
        let dirs = b"/w/sub\0".to_vec();
        let files = b"/w/b with space\0/w/a\nnewline\0".to_vec();
        let entries = merge_find_entries(root, &dirs, &files);
        let names: Vec<String> = entries
            .iter()
            .map(|e| e.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(names, vec!["/w/a\nnewline", "/w/b with space", "/w/sub"]);
        assert_eq!(entries.iter().filter(|e| e.is_dir).count(), 1);
    }

    #[test]
    fn empty_find_output_yields_no_entries() {
        let entries = merge_find_entries(Path::new("/w"), b"", b"");
        assert!(entries.is_empty());
    }

    #[test]
    fn container_request_sets_fields() {
        let r = container_request("cat", vec!["/f".to_string()]);
        assert_eq!(r.program, "cat");
        assert_eq!(r.args, vec!["/f".to_string()]);
    }
}

// ── Scripted-fake tests: the full FileSystemProvider impl without docker ──

#[cfg(test)]
mod fake_world_tests {
    use super::*;
    use shannon_tool_interface::{
        CapturedOutput, ExecCaps, PipedChild, PipedSpawn, ProcessExit, ProcessProvider,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// In-memory container image: paths → contents, plus directories.
    /// The fake answers the exact docker argv DockerExecFs composes, so the
    /// whole provider impl is exercised without a docker daemon.
    struct FakeDocker {
        files: Mutex<HashMap<&'static str, &'static [u8]>>,
        dirs: &'static [&'static str],
        writes: Mutex<Vec<String>>,
        renames: Mutex<Vec<(String, String)>>,
    }

    impl FakeDocker {
        fn image() -> Self {
            Self {
                files: Mutex::new(HashMap::from([
                    ("/w/README.md", &b"hello container"[..]),
                    ("/w/src/main.rs", &b"fn main() {}"[..]),
                ])),
                dirs: &["/w", "/w/src"],
                writes: Mutex::new(Vec::new()),
                renames: Mutex::new(Vec::new()),
            }
        }

        fn captured(stdout: &[u8]) -> CapturedOutput {
            CapturedOutput {
                stdout: stdout.to_vec(),
                stderr: Vec::new(),
                exit: ProcessExit::from_code(0),
            }
        }
    }

    #[async_trait]
    impl ProcessProvider for FakeDocker {
        #[allow(clippy::needless_return)] // arms mix early returns and tail expressions
        fn run_blocking(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
            let argv: Vec<String> = std::iter::once(request.program.clone())
                .chain(request.args.iter().cloned())
                .collect();
            // Answer the composed `docker exec ... <op>` argv by op name.
            let op = argv.iter().position(|a| {
                matches!(
                    a.as_str(),
                    "cat" | "find" | "stat" | "mv" | "head" | "readlink" | "mkdir" | "rm" | "sh"
                )
            });
            let Some(op_idx) = op else {
                return Ok(Self::captured(b""));
            };
            let tail = &argv[op_idx..];
            match tail[0].as_str() {
                "cat" => {
                    if tail.len() >= 3 && tail[1] == "-c" {
                        // write path: cat -c variant (unused today; sh handles it)
                        let path = tail.last().cloned().unwrap_or_default();
                        if let Some(data) = &request.stdin_data {
                            self.writes
                                .lock()
                                .unwrap()
                                .push(format!("{path}={}", String::from_utf8_lossy(data)));
                        }
                        return Ok(Self::captured(b""));
                    }
                    let path = tail[1].clone();
                    let files = self.files.lock().unwrap();
                    Ok(match files.get(path.as_str()) {
                        Some(bytes) => Self::captured(bytes),
                        None => CapturedOutput {
                            stdout: Vec::new(),
                            stderr: b"cat: No such file".to_vec(),
                            exit: ProcessExit::from_code(1),
                        },
                    })
                }
                "find" => {
                    let dir = tail[1].clone();
                    let is_dir_query = tail.contains(&"-type".to_string())
                        && tail
                            .iter()
                            .zip(tail.iter().skip(1))
                            .any(|(a, b)| a == "-type" && b == "d");
                    let mut out = Vec::new();
                    if is_dir_query {
                        for d in self.dirs {
                            if d.starts_with(dir.as_str()) && *d != dir {
                                out.extend_from_slice(d.as_bytes());
                                out.push(0);
                            }
                        }
                    } else {
                        let files = self.files.lock().unwrap();
                        for k in files.keys() {
                            if k.starts_with(dir.as_str()) {
                                out.extend_from_slice(k.as_bytes());
                                out.push(0);
                            }
                        }
                    }
                    return Ok(Self::captured(&out));
                }
                "stat" => {
                    // tail = [stat, -c, <fmt>, <path>]
                    let path = tail[3].clone();
                    let files = self.files.lock().unwrap();
                    let line = if let Some(bytes) = files.get(path.as_str()) {
                        format!("{} 1700000000 regular file", bytes.len())
                    } else if self.dirs.contains(&path.as_str()) {
                        "4096 1700000000 directory".to_string()
                    } else {
                        return Ok(CapturedOutput {
                            stdout: Vec::new(),
                            stderr: b"stat: can't find".to_vec(),
                            exit: ProcessExit::from_code(1),
                        });
                    };
                    return Ok(Self::captured(line.as_bytes()));
                }
                "mv" => {
                    let (from, to) = (tail[2].clone(), tail[3].clone());
                    self.renames.lock().unwrap().push((from, to));
                    return Ok(Self::captured(b""));
                }
                "head" => {
                    // head -c <n> <path>
                    let n: usize = tail[2].parse().unwrap_or(0);
                    let path = tail[3].clone();
                    let files = self.files.lock().unwrap();
                    let bytes = files
                        .get(path.as_str())
                        .map(|b| b[..n.min(b.len())].to_vec());
                    return Ok(Self::captured(&bytes.unwrap_or_default()));
                }
                "readlink" => {
                    let path = tail[2].clone();
                    return Ok(Self::captured(path.as_bytes()));
                }
                "mkdir" | "rm" => return Ok(Self::captured(b"")),
                "sh" => {
                    // write path: tail must be exactly
                    // [sh, -c, script, sh($0), <path>] — a clobbered argv
                    // (args lost the -c/script prefix) must fail loudly.
                    if tail.len() == 5 && tail[1] == "-c" && tail[3] == "sh" {
                        let path = tail[4].clone();
                        if let Some(data) = &request.stdin_data {
                            self.writes
                                .lock()
                                .unwrap()
                                .push(format!("{path}={}", String::from_utf8_lossy(data)));
                        }
                        return Ok(Self::captured(b""));
                    }
                    return Ok(CapturedOutput {
                        stdout: Vec::new(),
                        stderr: b"sh: malformed write argv".to_vec(),
                        exit: ProcessExit::from_code(2),
                    });
                }
                _ => return Ok(Self::captured(b"")),
            }
        }

        async fn run_async(&self, request: &ProcessRequest) -> io::Result<CapturedOutput> {
            self.run_blocking(request)
        }

        async fn spawn_piped(&self, _spec: &PipedSpawn) -> io::Result<Box<dyn PipedChild>> {
            Err(io::Error::other("fake has no children"))
        }

        fn capabilities(&self) -> ExecCaps {
            ExecCaps { is_remote: true }
        }
    }

    fn fs_with_fake() -> Arc<DockerExecFs> {
        DockerExecFs::new(DockerExecProcess::new(
            "it",
            PathBuf::from("/w"),
            Arc::new(FakeDocker::image()),
        ))
    }

    #[test]
    fn read_write_roundtrip_through_fake_container() {
        let fs = fs_with_fake();
        assert_eq!(
            fs.read_text_blocking(Path::new("/w/README.md")).unwrap(),
            "hello container"
        );
        fs.write_bytes_blocking(Path::new("/w/new.txt"), b"payload".as_slice())
            .unwrap();
    }

    #[test]
    fn list_dir_merges_files_and_dirs_sorted() {
        let fs = fs_with_fake();
        let entries = fs.list_dir_blocking(Path::new("/w")).unwrap();
        let paths: Vec<String> = entries
            .iter()
            .map(|e| e.path.to_string_lossy().to_string())
            .collect();
        assert_eq!(paths, vec!["/w/README.md", "/w/src", "/w/src/main.rs"]);
        assert!(entries.iter().filter(|e| e.is_dir).count() >= 1);
    }

    #[test]
    fn metadata_maps_stat_fields() {
        let fs = fs_with_fake();
        let md = fs.metadata_blocking(Path::new("/w/README.md")).unwrap();
        assert_eq!(md.len, 15);
        assert!(!md.is_dir);
        let dir = fs.metadata_blocking(Path::new("/w/src")).unwrap();
        assert!(dir.is_dir);
    }

    #[test]
    fn read_prefix_bounds_transfer() {
        let fs = fs_with_fake();
        let prefix = fs
            .read_prefix_blocking(Path::new("/w/README.md"), 5)
            .unwrap();
        assert_eq!(prefix, b"hello");
    }

    #[tokio::test]
    async fn rename_forwards_mv_f() {
        let fs = fs_with_fake();
        fs.rename(Path::new("/w/a"), Path::new("/w/b"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn async_faces_match_blocking() {
        let fs = fs_with_fake();
        assert_eq!(
            fs.read_text(Path::new("/w/README.md")).await.unwrap(),
            "hello container"
        );
        assert!(fs.metadata(Path::new("/w/src")).await.unwrap().is_dir);
    }
}
// ── Real-container integration (requires local docker + shannon-it) ─────

#[cfg(test)]
mod docker_it_tests {
    use super::*;

    // Ignored integration test: requires the docker daemon and a running
    // container named `shannon-it` (alpine works: busybox covers the ops).
    #[tokio::test]
    #[ignore = "requires local docker with a running `shannon-it` container"]
    async fn docker_fs_roundtrip_on_shannon_it() {
        let local = shannon_core_process();
        let proc = DockerExecProcess::new("shannon-it", PathBuf::from("/workspace"), local);
        let fs = DockerExecFs::new(proc);

        let file = PathBuf::from("/workspace/shannon-roundtrip.txt");
        fs.write_bytes(&file, b"hello from shannon").await.unwrap();
        assert_eq!(fs.read_text(&file).await.unwrap(), "hello from shannon");

        let md = fs.metadata(&file).await.unwrap();
        assert_eq!(md.len, 18, "busybox stat -c %s");
        assert!(!md.is_dir);

        let entries = fs.list_dir_blocking(Path::new("/workspace")).unwrap();
        assert!(entries.iter().any(|e| e.path == file));

        // Overwriting rename via mv -f.
        let dst = PathBuf::from("/workspace/shannon-roundtrip-dst.txt");
        fs.write_bytes(&dst, b"old").await.unwrap();
        fs.write_bytes(&file, b"new").await.unwrap();
        fs.rename(&file, &dst).await.unwrap();
        assert_eq!(fs.read_text(&dst).await.unwrap(), "new");

        let prefix = fs.read_prefix_blocking(&dst, 3).unwrap();
        assert_eq!(prefix, b"new");

        fs.remove_file_blocking(&dst).unwrap();
    }

    fn shannon_core_process() -> std::sync::Arc<dyn shannon_tool_interface::ProcessProvider> {
        let provider: std::sync::Arc<dyn shannon_tool_interface::ProcessProvider> =
            std::sync::Arc::new(shannon_core::providers::LocalProcess::new());
        provider
    }
}
