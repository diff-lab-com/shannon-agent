//! File permission helpers — ensure secret-bearing config files are
//! readable only by the owner.
//!
//! On Unix, sets the file mode to 0600 after write. On Windows, this is
//! a no-op (the file inherits ACL from the parent directory, which is
//! typically user-scoped under `%APPDATA%`).

use std::path::Path;

/// Set restrictive permissions on a file containing secrets.
///
/// - Unix: chmod 0600 (owner read/write only).
/// - Windows: no-op (NTFS ACL inherited from parent).
///
/// Errors are logged but not returned — a permission failure should not
/// abort a save that already wrote the file, since the secret is on disk
/// either way and the user can fix perms themselves.
pub fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(meta) => {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                if let Err(e) = std::fs::set_permissions(path, perms) {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "failed to set 0600 on secret-bearing file"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "could not stat file before restrict_to_owner"
                );
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}
