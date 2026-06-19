// Suppress lints that conflict with rustfmt or are style preferences from newer clippy.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]

use std::path::{Path, PathBuf};

pub mod config;
pub mod events;
pub mod extensions;
pub mod mcp;

/// Resolve `path` relative to `working_dir` (or use it as-is if absolute),
/// then canonicalize both and ensure the resolved path is inside the working
/// directory. Rejects path traversal (`..`), absolute paths outside the
/// working dir, and symlinks that escape the working dir.
///
/// Returns the canonicalized path on success. The helper is fallible by
/// design: callers translate the `Err(String)` into their own error type.
///
/// Used to harden IPC commands that accept user-supplied file paths — a
/// compromised frontend must not be able to read/write arbitrary files
/// (e.g. `~/.ssh/id_rsa`, `~/.shannon/desktop/config.json`).
pub(crate) fn resolve_path_in_working_dir(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, String> {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };
    // `canonicalize` resolves `..`, symlinks, and case-insensitive roots.
    // We require the target to exist so callers get a meaningful "not found"
    // error before any write attempt; commands that need to create new files
    // should canonicalize the parent directory instead.
    let canonical = resolved
        .canonicalize()
        .map_err(|e| format!("path not found: {e}"))?;
    let canonical_cwd = working_dir
        .canonicalize()
        .map_err(|e| format!("invalid working directory: {e}"))?;
    if !canonical.starts_with(&canonical_cwd) {
        return Err(format!(
            "path '{}' is outside the working directory",
            canonical.display()
        ));
    }
    Ok(canonical)
}

/// Validate that `path`'s *parent* directory is inside `working_dir`, without
/// requiring `path` to exist yet. Use this for write targets (e.g. file
/// creation) where the file itself does not exist on entry. Returns the
/// canonicalized parent directory plus the joined file path on success.
#[allow(dead_code)]
pub(crate) fn resolve_write_target_in_working_dir(
    path: &str,
    working_dir: &Path,
) -> Result<PathBuf, String> {
    let resolved = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        working_dir.join(path)
    };
    let parent = resolved
        .parent()
        .ok_or_else(|| format!("path has no parent: {path}"))?;
    if parent.as_os_str().is_empty() {
        return Err(format!("path '{path}' has no parent directory"));
    }
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("parent dir not found: {e}"))?;
    let canonical_cwd = working_dir
        .canonicalize()
        .map_err(|e| format!("invalid working directory: {e}"))?;
    if !canonical_parent.starts_with(&canonical_cwd) {
        return Err(format!(
            "path '{}' is outside the working directory",
            resolved.display()
        ));
    }
    // Re-join the file name onto the canonicalized parent so the caller gets a
    // usable absolute path (with the original file name, uncanonicalized).
    let file_name = resolved
        .file_name()
        .ok_or_else(|| format!("path has no file name: {path}"))?;
    Ok(canonical_parent.join(file_name))
}

#[cfg(feature = "tauri")]
pub mod commands;

#[cfg(feature = "tauri")]
pub mod commands_mcp;

#[cfg(feature = "tauri")]
pub mod scheduled_commands;

#[cfg(feature = "tauri")]
pub mod lsp_commands;

#[cfg(feature = "tauri")]
pub mod automation_commands;

#[cfg(feature = "tauri")]
pub mod extensions_commands;

#[cfg(feature = "tauri")]
pub mod notifications;

#[cfg(feature = "tauri")]
pub mod inbound;
