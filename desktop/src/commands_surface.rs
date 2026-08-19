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

// ── C1①: semi-automatic update check ────────────────────────────────
//
// The full in-place updater needs signing + a latest.json channel
// (ADR-0011 open question, scheduled with C4). Until then the app offers
// a check-then-open-the-download-page flow that only needs the public
// GitHub API.

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdateInfo {
    pub current_version: String,
    /// Latest release tag (`vX.Y.Z`), when the check succeeded.
    pub latest_version: Option<String>,
    pub update_available: bool,
    /// The release page to open in a browser.
    pub release_url: String,
    /// Why the check failed, when it did (rendered as a hint, not an error).
    pub error: Option<String>,
}

/// Numeric dot-version compare — `latest > current`. Same lenient parsing
/// as the CLI's `version_is_newer`: strips a leading `v`, ignores
/// non-numeric suffixes, missing components count as 0.
fn version_is_newer(current: &str, latest: &str) -> bool {
    fn parse(v: &str) -> Vec<u64> {
        v.split('.')
            .map(|p| {
                p.trim_start_matches('v')
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("")
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    }
    let a = parse(current);
    let b = parse(latest);
    let len = a.len().max(b.len());
    for i in 0..len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if y > x {
            return true;
        }
        if y < x {
            return false; // first differing component decides
        }
    }
    false
}

/// C1①: check GitHub for a newer release. Never fails the command —
/// network problems land in `error` so the UI can show a soft hint.
#[tauri::command]
pub async fn check_app_update() -> Result<AppUpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let mut info = AppUpdateInfo {
        current_version: current.clone(),
        latest_version: None,
        update_available: false,
        release_url: "https://github.com/diff-lab-com/shannon-agent/releases".to_string(),
        error: None,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let resp = match client
        .get("https://api.github.com/repos/diff-lab-com/shannon-agent/releases/latest")
        .header("User-Agent", "shannon-desktop")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            info.error = Some(format!("network error: {e}"));
            return Ok(info);
        }
    };
    if !resp.status().is_success() {
        info.error = Some(format!("GitHub returned HTTP {}", resp.status()));
        return Ok(info);
    }
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            info.error = Some(format!("bad release metadata: {e}"));
            return Ok(info);
        }
    };
    if let Some(url) = body.get("html_url").and_then(|u| u.as_str()) {
        info.release_url = url.to_string();
    }
    match body.get("tag_name").and_then(|t| t.as_str()) {
        Some(tag) => {
            info.update_available = version_is_newer(&current, tag);
            info.latest_version = Some(tag.to_string());
        }
        None => info.error = Some("no tag_name in release metadata".to_string()),
    }
    Ok(info)
}

/// C1①: open the release page in the system browser — same shell-open
/// precedent as the OAuth flow in extensions_commands.rs.
#[tauri::command]
pub async fn open_release_page(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    #[allow(deprecated)]
    app.shell()
        .open(url, None)
        .map_err(|e| format!("failed to open browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::version_is_newer;

    #[test]
    fn detects_newer_patch_minor_major() {
        assert!(version_is_newer("0.11.0", "0.11.1"));
        assert!(version_is_newer("0.11.0", "0.12.0"));
        assert!(version_is_newer("0.11.0", "1.0.0"));
    }

    #[test]
    fn equal_or_older_is_not_newer() {
        assert!(!version_is_newer("0.11.0", "0.11.0"));
        assert!(!version_is_newer("0.11.1", "0.11.0"));
        // A higher major must dominate later components (1.0 > 0.9).
        assert!(!version_is_newer("1.0", "0.9"));
        // Lenient parse (same as the CLI): "-rc.1" reads as an extra `.1`
        // component, so prerelease tags compare as newer. Acceptable —
        // /releases/latest never serves prereleases.
        assert!(version_is_newer("0.11.0", "0.11.0-rc.1"));
    }

    #[test]
    fn tolerates_v_prefix_and_ragged_lengths() {
        assert!(version_is_newer("0.11", "v0.12"));
        assert!(version_is_newer("0.11.0", "0.12")); // missing parts are 0
        assert!(!version_is_newer("0.11.0", "0.11.0.0"));
    }
}
