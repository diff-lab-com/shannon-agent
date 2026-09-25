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

/// The only URL this command may open: an https URL on the official repo
/// domain (repo root, releases tree — covers `/releases`, `/releases/tag/…`,
/// `/blob/…` etc.).
fn is_official_release_url(url: &str) -> bool {
    let parsed = url::Url::parse(url);
    match parsed {
        Ok(u) => {
            u.scheme() == "https"
                && u.host_str() == Some("github.com")
                && u.path().starts_with("/diff-lab-com/shannon-agent")
        }
        Err(_) => false,
    }
}

/// C1①: open the release page in the system browser — same shell-open
/// precedent as the OAuth flow in extensions_commands.rs.
///
/// Review §P3 (桌面): the URL comes back from the webview, i.e. from a
/// potentially compromised renderer — it must not be opened unless it
/// points at the official repository domain, otherwise the command is an
/// arbitrary-URL opener (phishing / command-prompt abuse via `shell::open`).
#[tauri::command]
pub async fn open_release_page(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    if !is_official_release_url(&url) {
        tracing::warn!(url = %url, "open_release_page rejected non-official URL");
        return Err(format!("refusing to open non-official URL: {url}"));
    }
    #[allow(deprecated)]
    app.shell()
        .open(url, None)
        .map_err(|e| format!("failed to open browser: {e}"))
}

// ---------------------------------------------------------------------------
// External open pipeline (2026-09-25 design doc §4 P0-A / P1-D / P1-E).
//
// `open_release_page` above deliberately whitelists a single domain; the
// chat pipeline needs the general case, so the posture shifts from
// "whitelist the URL" to "whitelist the scheme + scope the paths" (§5
// decision 1): only http/https URLs leave the app, and path commands only
// act inside $HOME/** / $TEMP/** — the same scope the asset protocol
// already grants the webview.
// ---------------------------------------------------------------------------

/// Decision §5-1: only http/https URLs may be opened. Every other scheme
/// (`file:`, `javascript:`, custom app handlers) is rejected so a
/// compromised renderer cannot turn this command into an arbitrary opener.
fn is_openable_url(url: &str) -> bool {
    match url::Url::parse(url) {
        Ok(u) => matches!(u.scheme(), "http" | "https") && u.host_str().is_some(),
        Err(_) => false,
    }
}

/// Path roots the external-open commands may touch. Mirrors the asset
/// protocol scope in tauri.conf.json (`$HOME/**`, `$TEMP/**`). Bases are
/// canonicalized the same way the probed path will be — macOS resolves
/// `/var` → `/private/var` inside `std::env::temp_dir()`, and Windows
/// `canonicalize` returns `\\?\`-prefixed verbatim paths, both of which
/// would otherwise never `starts_with` the raw base (§review P1-3).
pub(crate) fn allowed_path_bases() -> Vec<std::path::PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = dirs::home_dir() {
        bases.push(normalized_base(&home));
    }
    bases.push(normalized_base(&std::env::temp_dir()));
    bases
}

fn normalized_base(base: &std::path::Path) -> std::path::PathBuf {
    let canonical = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    strip_windows_verbatim(&canonical)
}

fn strip_windows_verbatim(p: &std::path::Path) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        p.as_os_str()
            .to_string_lossy()
            .strip_prefix(r"\\?\")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| p.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

/// Strict scope check for commands that *act* on a path (open / reveal /
/// read): the path must exist and canonicalize inside an allowed base, so
/// both `..` segments and symlink escapes fail.
pub(crate) fn canonicalized_in_scope(path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return Err(format!("path must be absolute: {path}"));
    }
    let canonical = p
        .canonicalize()
        .map_err(|e| format!("path not accessible: {path}: {e}"))?;
    let canonical = strip_windows_verbatim(&canonical);
    if allowed_path_bases().iter().any(|base| canonical.starts_with(base)) {
        Ok(canonical)
    } else {
        Err(format!(
            "path outside allowed scope ($HOME/**, $TEMP/**): {path}"
        ))
    }
}

/// Lexical scope check for pure existence probes: the probed path may not
/// exist (that is what the probe is for), so there is nothing to
/// canonicalize — reject non-absolute paths and any `..` traversal, then
/// let the caller test the filesystem. Content is never returned through
/// this path, so a symlink escape only leaks a boolean.
pub(crate) fn is_probable_path_in_scope(path: &str) -> bool {
    let p = std::path::Path::new(path);
    if !p.is_absolute() {
        return false;
    }
    if p.components().any(|c| c == std::path::Component::ParentDir) {
        return false;
    }
    allowed_path_bases().iter().any(|base| p.starts_with(base))
}

/// Open an http/https URL in the system browser (chat links, web tabs).
#[tauri::command]
pub async fn open_external(app: tauri::AppHandle, url: String) -> Result<(), String> {
    if !is_openable_url(&url) {
        tracing::warn!(url = %url, "open_external rejected non-http(s) URL");
        return Err(format!("refusing to open non-http(s) URL: {url}"));
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("failed to open URL: {e}"))
}

/// Open a local file with its OS default application.
#[tauri::command]
pub async fn open_with_default_app(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let canonical = canonicalized_in_scope(&path)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(canonical.to_string_lossy(), None::<&str>)
        .map_err(|e| format!("failed to open path: {e}"))
}

/// Show a local file in the OS file manager.
#[tauri::command]
pub async fn reveal_in_folder(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let canonical = canonicalized_in_scope(&path)?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .reveal_item_in_dir(&canonical)
        .map_err(|e| format!("failed to reveal path: {e}"))
}

const ARTIFACT_EXPORT_EXTS: [&str; 8] = ["md", "markdown", "html", "htm", "svg", "mmd", "mermaid", "txt"];
const MAX_ARTIFACT_EXPORT_BYTES: usize = 2 * 1024 * 1024;

fn slugify_title(title: &str) -> String {
    let mut slug = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "artifact".to_string()
    } else {
        slug.chars().take(48).collect()
    }
}

/// Deterministic temp filename so re-exporting the same artifact rewrites
/// its file instead of littering $TEMP with copies.
fn artifact_temp_file_name(title: &str, source: &str, ext: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{}-{:08x}.{ext}", slugify_title(title), hasher.finish() as u32)
}

/// P1-D escape hatch: render HTML (or other text artifacts) where the OS
/// renders it best — write to $TEMP, then hand to the default application.
/// Returns the temp path that was opened.
#[tauri::command]
pub async fn open_artifact_externally(
    app: tauri::AppHandle,
    title: String,
    source: String,
    ext: String,
) -> Result<String, String> {
    let ext = ext.to_ascii_lowercase();
    if !ARTIFACT_EXPORT_EXTS.contains(&ext.as_str()) {
        return Err(format!("unsupported artifact extension: {ext}"));
    }
    if source.len() > MAX_ARTIFACT_EXPORT_BYTES {
        return Err(format!(
            "artifact too large to export: {} bytes",
            source.len()
        ));
    }
    let dir = std::env::temp_dir().join("shannon-artifacts");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| format!("failed to create temp dir: {e}"))?;
    let path = dir.join(artifact_temp_file_name(&title, &source, &ext));
    tokio::fs::write(&path, &source)
        .await
        .map_err(|e| format!("failed to write temp artifact: {e}"))?;
    let path_str = path.to_string_lossy().into_owned();
    open_with_default_app(app, path_str.clone()).await?;
    Ok(path_str)
}

/// P1-E: the web tab probes the target server-side because X-Frame-Options
/// / frame-ancestors rejections cannot be detected from inside a
/// cross-origin iframe — this turns a blank frame into an immediate
/// "open in browser" fallback card.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameProbe {
    pub frameable: bool,
    pub status: u16,
    pub reason: Option<String>,
}

/// Pure header inspection for `probe_url_frameable`, split out for tests.
/// Reads every CSP header (a site may emit several; the strictest wins) —
/// `frame-ancestors` with any explicit list is treated as "not frameable"
/// (the app origin is never on a third party's allowlist; `*` and absence
/// of the directive frame freely).
fn headers_allow_framing(xfo: Option<&str>, csps: &[&str]) -> (bool, Option<String>) {
    if let Some(xfo) = xfo {
        let v = xfo.to_ascii_lowercase();
        if v.contains("deny") || v.contains("sameorigin") {
            return (false, Some(format!("x-frame-options: {xfo}")));
        }
    }
    for csp in csps {
        for directive in csp.split(';') {
            if let Some(rest) = directive.trim().strip_prefix("frame-ancestors") {
                let rest = rest.trim();
                if rest == "*" {
                    return (true, None);
                }
                return (false, Some(format!("frame-ancestors: {rest}")));
            }
        }
    }
    (true, None)
}

#[tauri::command]
pub async fn probe_url_frameable(url: String) -> Result<FrameProbe, String> {
    if !is_openable_url(&url) {
        return Err(format!("refusing to probe non-http(s) URL: {url}"));
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let resp = client
        .get(&url)
        .header("user-agent", "Mozilla/5.0 (Shannon artifact frame probe)")
        .send()
        .await;
    match resp {
        Ok(resp) => {
            let status = resp.status().as_u16();
            let header = |name: &str| -> Vec<String> {
                resp.headers()
                    .get_all(name)
                    .iter()
                    .filter_map(|v| v.to_str().ok())
                    .map(|s| s.to_string())
                    .collect()
            };
            let xfo = header("x-frame-options");
            let csps = header("content-security-policy");
            let xfo_ref: Option<&str> = xfo.first().map(|s| s.as_str());
            let csp_refs: Vec<&str> = csps.iter().map(|s| s.as_str()).collect();
            let (frameable, reason) = headers_allow_framing(xfo_ref, &csp_refs);
            Ok(FrameProbe {
                frameable,
                status,
                reason,
            })
        }
        // Transport failure: the embedded frame may still work (proxies,
        // login-gated hosts) — let the iframe try rather than pre-failing.
        Err(e) => Ok(FrameProbe {
            frameable: true,
            status: 0,
            reason: Some(format!("probe transport error: {e}")),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        artifact_temp_file_name,
        canonicalized_in_scope,
        headers_allow_framing,
        is_openable_url,
        is_probable_path_in_scope,
        is_official_release_url,
        slugify_title,
        version_is_newer,
        ARTIFACT_EXPORT_EXTS,
    };

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

    #[test]
    fn rejects_non_official_urls() {
        // Wrong scheme / host / path / parse garbage — all refused.
        assert!(!is_official_release_url(
            "http://github.com/diff-lab-com/shannon-agent/releases"
        ));
        assert!(!is_official_release_url(
            "https://evil.com/diff-lab-com/shannon-agent/releases"
        ));
        assert!(!is_official_release_url(
            "https://github.com.evil.com/diff-lab-com/shannon-agent/releases"
        ));
        assert!(!is_official_release_url(
            "https://github.com/other-org/shannon-agent/releases"
        ));
        assert!(!is_official_release_url("file:///etc/passwd"));
        assert!(!is_official_release_url("not a url"));
    }

    #[test]
    fn accepts_official_repo_urls() {
        assert!(is_official_release_url(
            "https://github.com/diff-lab-com/shannon-agent/releases"
        ));
        assert!(is_official_release_url(
            "https://github.com/diff-lab-com/shannon-agent/releases/tag/v0.12.0"
        ));
        assert!(is_official_release_url(
            "https://github.com/diff-lab-com/shannon-agent"
        ));
    }

    // -- open pipeline (§4 P0-A) -------------------------------------------

    #[test]
    fn openable_urls_are_http_https_with_host() {
        assert!(is_openable_url("https://example.com/docs"));
        assert!(is_openable_url("http://localhost:1420/preview"));
        assert!(!is_openable_url("file:///etc/passwd"));
        assert!(!is_openable_url("javascript:alert(1)"));
        assert!(!is_openable_url("ftp://example.com/a"));
        assert!(!is_openable_url("https:"));
        assert!(!is_openable_url("not a url"));
    }

    #[test]
    fn canonical_scope_rejects_traversal_and_outside_paths() {
        let base = std::env::temp_dir().join("shannon-scope-test");
        std::fs::create_dir_all(&base).unwrap();
        let inside = base.join("inside.txt");
        std::fs::write(&inside, "ok").unwrap();

        let got = canonicalized_in_scope(&inside.to_string_lossy());
        assert!(got.is_ok(), "temp file must be in scope: {got:?}");

        assert!(canonicalized_in_scope("relative/file.txt").is_err());
        assert!(canonicalized_in_scope("/etc/passwd").is_err());
        assert!(canonicalized_in_scope("/nonexistent-path-xyz").is_err());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn probable_scope_rejects_relative_and_traversal() {
        let inside = std::env::temp_dir().join("somewhere/file.md");
        assert!(is_probable_path_in_scope(&inside.to_string_lossy()));
        assert!(!is_probable_path_in_scope("relative/file.md"));
        let traversal = std::env::temp_dir()
            .join("a/../../etc/passwd")
            .to_string_lossy()
            .into_owned();
        assert!(!is_probable_path_in_scope(&traversal));
    }

    // -- artifact export (§4 P1-D) ------------------------------------------

    #[test]
    fn artifact_ext_whitelist() {
        assert!(ARTIFACT_EXPORT_EXTS.contains(&"html"));
        assert!(!ARTIFACT_EXPORT_EXTS.contains(&"exe"));
        assert!(!ARTIFACT_EXPORT_EXTS.contains(&"pdf"));
    }

    #[test]
    fn slugify_takes_ascii_alnum_only() {
        assert_eq!(slugify_title("My Report!"), "my_report");
        assert_eq!(slugify_title("报告 文档"), "artifact");
        assert_eq!(slugify_title("---"), "artifact");
        assert_eq!(slugify_title("A B").len(), 3);
    }

    #[test]
    fn artifact_temp_name_is_deterministic_per_source() {
        let a = artifact_temp_file_name("Report", "content-a", "html");
        let b = artifact_temp_file_name("Report", "content-a", "html");
        let c = artifact_temp_file_name("Report", "content-b", "html");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.ends_with(".html"));
    }

    // -- web tab framing probe (§4 P1-E) ------------------------------------

    #[test]
    fn xfo_headers_block_framing() {
        let (frameable, reason) = headers_allow_framing(Some("DENY"), &[]);
        assert!(!frameable);
        assert!(reason.unwrap().contains("DENY"));

        let (frameable, _) = headers_allow_framing(Some("SAMEORIGIN"), &[]);
        assert!(!frameable);

        let (frameable, _) = headers_allow_framing(Some("allow-from https://a"), &[]);
        assert!(frameable, "legacy allow-from is not the common block case");
    }

    #[test]
    fn csp_frame_ancestors_blocks_framing_unless_star() {
        let (frameable, _) = headers_allow_framing(
            None,
            &["default-src 'self'; frame-ancestors 'none'; script-src 'self'"],
        );
        assert!(!frameable);

        let (frameable, _) = headers_allow_framing(
            None,
            &["default-src 'self'; frame-ancestors https://partner.example"],
        );
        assert!(!frameable);

        let (frameable, _) = headers_allow_framing(
            None,
            &["default-src 'self'; frame-ancestors *; script-src 'self'"],
        );
        assert!(frameable);
    }

    #[test]
    fn second_csp_header_with_ancestors_still_blocks() {
        let (frameable, _) = headers_allow_framing(
            None,
            &["default-src 'self'", "frame-ancestors 'none'"],
        );
        assert!(!frameable);
    }

    #[test]
    fn no_framing_headers_means_frameable() {
        let (frameable, reason) = headers_allow_framing(None, &[]);
        assert!(frameable);
        assert!(reason.is_none());

        let (frameable, _) = headers_allow_framing(None, &["default-src 'self'"]);
        assert!(frameable);
    }
}
