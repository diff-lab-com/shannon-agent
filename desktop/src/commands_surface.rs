//! Tauri commands for surface identity (B7) and in-app CLI installation (B3).
//!
//! ADR-0011 Phase B: the desktop bundle ships the `shannon` CLI alongside
//! the GUI (Tauri externalBin). The macOS dmg cannot touch PATH at install
//! time, so the app offers a VS Code-style "install `shannon` in PATH"
//! action; the deb/rpm packages and the NSIS installer hook already handle
//! it at package time.

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInfo {
    pub surface: &'static str,
    pub version: &'static str,
}

/// B7: self-identify this surface (routing / telemetry / support). The
/// version is the desktop crate version, which release-prep keeps in
/// lockstep with tauri.conf.json and the workspace (CLI) version.
#[tauri::command]
pub async fn get_surface_info() -> Result<SurfaceInfo, String> {
    Ok(SurfaceInfo {
        surface: "desktop",
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallStatus {
    /// `shannon` currently resolves on PATH (the copy that wins).
    pub on_path: bool,
    /// Version reported by the PATH-resolvable binary, when present.
    pub on_path_version: Option<String>,
    /// The CLI bundled with this desktop install (externalBin), when found.
    pub bundled_path: Option<String>,
    /// True when the platform installer already handles PATH registration
    /// (deb/rpm → /usr/bin, NSIS hook) and the button is informational.
    pub handled_by_installer: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInstallResult {
    pub status: CliInstallStatus,
    /// Where the symlink landed after a successful in-app install.
    pub installed_link: Option<String>,
    /// Human-readable explanation of what was (or wasn't) done.
    pub message: String,
}

/// Locate the CLI bundled next to this executable — Tauri places
/// externalBin files alongside the main binary (.app Contents/MacOS,
/// deb/rpm /usr/bin, NSIS $INSTDIR).
fn bundled_cli_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) {
        "shannon.exe"
    } else {
        "shannon"
    };
    let sibling = exe.parent()?.join(name);
    sibling.is_file().then_some(sibling)
}

/// First `shannon` found on PATH (direct walk — `command -v` is a shell
/// builtin and must not be shelled out to).
fn shannon_on_path() -> Option<std::path::PathBuf> {
    let name = if cfg!(windows) {
        "shannon.exe"
    } else {
        "shannon"
    };
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// First whitespace-separated token that starts with a digit —
/// `shannon 0.11.0` → `0.11.0`.
fn probe_version(binary: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(String::from)
}

fn current_status() -> CliInstallStatus {
    let on_path = shannon_on_path();
    CliInstallStatus {
        on_path_version: on_path.as_deref().and_then(probe_version),
        on_path: on_path.is_some(),
        bundled_path: bundled_cli_path().map(|p| p.display().to_string()),
        handled_by_installer: cfg!(any(target_os = "linux", target_os = "windows")),
    }
}

/// B3: report whether the bundled `shannon` is reachable from a shell.
#[tauri::command]
pub async fn get_cli_install_status() -> Result<CliInstallStatus, String> {
    Ok(current_status())
}

/// B3: expose the bundled `shannon` on PATH.
///
/// Non-shadowing (same rule as the NSIS hook): when a `shannon` already
/// resolves on PATH — install.sh, brew, a previous link — nothing is
/// touched. Otherwise:
///   macOS/linux → symlink /usr/local/bin/shannon → bundled binary,
///                 falling back to ~/.local/bin/shannon;
///   windows     → informational only (the NSIS hook already registered
///                 $INSTDIR on the user PATH at install time).
#[tauri::command]
pub async fn install_cli_to_path() -> Result<CliInstallResult, String> {
    let bundled = match bundled_cli_path() {
        Some(p) => p,
        None => {
            return Ok(CliInstallResult {
                status: current_status(),
                installed_link: None,
                message: "no bundled CLI found next to the desktop binary".to_string(),
            });
        }
    };

    if let Some(existing) = shannon_on_path() {
        let version = probe_version(&existing);
        return Ok(CliInstallResult {
            status: current_status(),
            installed_link: None,
            message: format!(
                "shannon is already on PATH ({}{}) — left unchanged",
                existing.display(),
                version.map(|v| format!(", {v}")).unwrap_or_default()
            ),
        });
    }

    match try_link_on_unix(&bundled) {
        Ok(target) => Ok(CliInstallResult {
            status: current_status(),
            installed_link: Some(target.display().to_string()),
            message: format!("linked {} -> {}", target.display(), bundled.display()),
        }),
        Err(msg) => Ok(CliInstallResult {
            status: current_status(),
            installed_link: None,
            message: msg,
        }),
    }
}

/// Create the symlink on unix; on windows the installer hook owns PATH.
#[cfg(unix)]
fn try_link_on_unix(bundled: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::os::unix::fs::symlink;
    let mut targets = vec![std::path::PathBuf::from("/usr/local/bin/shannon")];
    if let Some(home) = dirs::home_dir() {
        targets.push(home.join(".local").join("bin").join("shannon"));
    }
    let mut last_err = String::new();
    for target in targets {
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // Replace only our own previous symlink — never a real file.
        if target
            .symlink_metadata()
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            let _ = std::fs::remove_file(&target);
        } else if target.exists() {
            continue; // a real binary owns this slot — skip, never shadow
        }
        match symlink(bundled, &target) {
            Ok(()) => return Ok(target),
            Err(e) => last_err = format!("symlink to {} failed: {e}", target.display()),
        }
    }
    Err(format!(
        "could not create the symlink (tried /usr/local/bin and ~/.local/bin): {last_err}"
    ))
}

#[cfg(windows)]
fn try_link_on_unix(_bundled: &std::path::Path) -> Result<std::path::PathBuf, String> {
    Err("handled by the installer (open a new terminal, or re-run the setup)".to_string())
}
