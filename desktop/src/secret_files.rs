//! Unified writer for plaintext-secret-bearing config files: atomic replace
//! with owner-only permissions applied **before** any secret bytes reach
//! disk (R6 immediate item; adversarial-review A1 — one helper for every
//! secret file, never per-file patches).
//!
//! Why the temp file is created at `0600` on Unix: the naive
//! `fs::write(tmp, ..)` + `chmod`-afterwards sequence leaves a window in
//! which the plaintext (OAuth tokens, IMAP passwords, webhook secrets, …)
//! sits world-readable at the umask default (`0644`). Here the temp file is
//! created `0600` *first* (creation mode, no window), the content is written
//! into it, and only then is it renamed over the target — so the target is
//! `0600` from the instant it exists. Pre-existing `0644` files are fixed as
//! a side effect of the next write (the rename swaps in the new inode); we
//! deliberately do **not** batch-chmod old files outside this path.
//!
//! Known limitations (declared, per the R6 ruling):
//!
//! * `0600` only fences off *other local users*. It does not protect
//!   against backups, cloud-sync folders, filesystem snapshots, or a machine
//!   without full-disk encryption — anyone holding the FDE key or a copy of
//!   a backup reads the plaintext.
//! * The complete fix is moving these secrets into the OS keychain
//!   (planned batch F5, a later iteration — not this branch).
//!
//! Windows: no ACL work is attempted, and none is pretended. Files created
//! under the user profile inherit the user-scoped DACL of that directory by
//! default, which is the honest status quo; tightening ACLs explicitly is
//! out of scope here.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Atomically replace `path` with `contents`, never letting a secret byte
/// touch disk at world-readable permissions (Unix).
///
/// Sequence: create a sibling temp file `.<name>.tmp` with mode `0600` at
/// creation time (Unix; `std::os::unix::fs::PermissionsExt` is used again
/// before the rename as belt-and-braces, since `chmod` — unlike the open()
/// creation mode — is never filtered by the umask), write the content into
/// it, then rename it over `path`. A crash mid-write can never leave a
/// truncated target, and a reader can never observe the plaintext at a
/// weaker mode. A leftover temp file from an earlier crash is removed and
/// the write retried once.
pub fn write_atomic_owner_only(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    // A bare relative file name ("x.toml") has an empty parent — write the
    // temp file into the current directory, matching `fs::write` semantics.
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    std::fs::create_dir_all(&parent)?;
    let file_name = path.file_name().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("secret file path has no file name: {}", path.display()),
        )
    })?;
    let tmp = parent.join(format!(".{}.tmp", file_name.to_string_lossy()));

    let mut file = match create_temp_owner_only(&tmp) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // Leftover temp from a crashed run — a fresh create_new() must
            // not be wedged by it. Remove and retry exactly once.
            std::fs::remove_file(&tmp)?;
            create_temp_owner_only(&tmp)?
        }
        Err(e) => return Err(e),
    };
    if let Err(e) = file.write_all(contents) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    drop(file);
    // Belt-and-braces: chmod (not umask-filtered) pins 0600 regardless of
    // what the creation mode ended up as. Cheap and unconditional.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Create the temp file exclusively. On Unix it is born `0600` (umask can
/// only strip the group/other bits we did not ask for anyway); on Windows it
/// is a plain exclusive create — no ACL is set (see the module docs).
fn create_temp_owner_only(tmp: &Path) -> std::io::Result<std::fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(tmp)
    }
    #[cfg(not(unix))]
    {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(tmp)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn file_mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .expect("file exists")
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    fn new_file_is_created_0600() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.json");
        write_atomic_owner_only(&path, b"{\"token\":\"secret\"}").expect("write");
        assert_eq!(file_mode(&path), 0o600);
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"{\"token\":\"secret\"}"
        );
    }

    #[test]
    fn preexisting_0644_file_becomes_0600_on_rewrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("config.json");
        std::fs::write(&path, b"{\"old\":true}").expect("seed file");
        // Pin the seed mode explicitly — the process umask varies.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("chmod 0644");
        assert_eq!(file_mode(&path), 0o644, "seed must start permissive");
        write_atomic_owner_only(&path, b"{\"token\":\"secret\"}").expect("rewrite");
        assert_eq!(file_mode(&path), 0o600);
        assert_eq!(
            std::fs::read(&path).expect("read"),
            b"{\"token\":\"secret\"}"
        );
    }

    #[test]
    fn leftover_temp_file_does_not_wedge_the_write() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.toml");
        let stale = dir.path().join(".secrets.toml.tmp");
        std::fs::write(&stale, b"junk from a crashed run").expect("seed stale temp");
        write_atomic_owner_only(&path, b"password = \"hunter2\"").expect("write");
        assert_eq!(file_mode(&path), 0o600);
        assert!(!stale.exists(), "stale temp must be gone after rename");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("data-sources").join("imap.toml");
        write_atomic_owner_only(&path, b"password = \"hunter2\"").expect("write");
        assert_eq!(file_mode(&path), 0o600);
    }
}
