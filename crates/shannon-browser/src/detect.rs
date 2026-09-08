//! System-browser detection (T14 Phase 1 foundation).
//!
//! Shannon never bundles a Chromium binary: browser automation reuses the
//! browser the user already installed (team decision 2026-09-06). This
//! module locates a Chrome/Chromium/Edge executable per platform and, when
//! none is found, reports every path it searched plus distro-appropriate
//! install hints so `/browser doctor` can render actionable guidance.

use std::path::{Path, PathBuf};

/// A located browser executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserExecutable {
    pub path: PathBuf,
    /// Where it came from ("env", "linux-path", "macos-app", "windows-path").
    pub source: &'static str,
}

/// Why detection failed, with everything tried — rendered by `/browser doctor`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectError {
    pub searched: Vec<PathBuf>,
}

impl std::fmt::Display for DetectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "No compatible browser found. Searched:")?;
        for p in &self.searched {
            writeln!(f, "  • {}", p.display())?;
        }
        write!(f, "{}", install_hint())
    }
}

/// Distro/package-manager install guidance for the doctor output.
pub fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "To enable browser control, install one of:\n  • brew: brew install --cask chromium\n  • or download Google Chrome from https://www.google.com/chrome/"
    } else if cfg!(target_os = "windows") {
        "To enable browser control, install one of:\n  • winget: winget install Google.Chrome\n  • or download Chrome from https://www.google.com/chrome/"
    } else {
        "To enable browser control, install one of:\n  • apt:    sudo apt install chromium-browser\n  • dnf:    sudo dnf install chromium\n  • pacman: sudo pacman -S chromium\n  • snap:   sudo snap install chromium\n  • Or use the Playwright MCP instead: /browser setup"
    }
}

/// `SHANNON_BROWSER_PATH` override — wins over every platform default.
fn env_override() -> Option<PathBuf> {
    std::env::var_os("SHANNON_BROWSER_PATH")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Candidate executables for the current platform, in priority order.
/// Pure listing (no filesystem access) so tests can assert platform shape.
pub fn candidate_paths() -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    if let Some(p) = env_override() {
        out.push((p, "env"));
    }
    if cfg!(target_os = "macos") {
        for base in ["/Applications", &format!("{}/Applications", home())] {
            for app in [
                "Google Chrome.app/Contents/MacOS/Google Chrome",
                "Chromium.app/Contents/MacOS/Chromium",
                "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            ] {
                out.push((PathBuf::from(base).join(app), "macos-app"));
            }
        }
        for p in ["/opt/homebrew/bin/chromium", "/usr/local/bin/chromium"] {
            out.push((PathBuf::from(p), "macos-app"));
        }
    } else if cfg!(target_os = "windows") {
        for base in [
            std::env::var("ProgramFiles").unwrap_or_default(),
            std::env::var("ProgramFiles(x86)").unwrap_or_default(),
            std::env::var("LOCALAPPDATA").unwrap_or_default(),
        ]
        .into_iter()
        .filter(|b| !b.is_empty())
        {
            for rel in [
                r"Google\Chrome\Application\chrome.exe",
                r"Chromium\Application\chrome.exe",
            ] {
                out.push((PathBuf::from(&base).join(rel), "windows-path"));
            }
        }
    } else {
        for p in [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/snap/bin/chromium",
            &format!("{}/.local/bin/chromium", home()),
        ] {
            out.push((PathBuf::from(p), "linux-path"));
        }
    }
    out
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.is_file()
            && std::fs::metadata(path)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Locate the first usable browser executable.
pub fn detect_system_browser() -> Result<BrowserExecutable, DetectError> {
    let candidates = candidate_paths();
    let searched: Vec<PathBuf> = candidates.iter().map(|(p, _)| p.clone()).collect();
    for (path, source) in candidates {
        // `env` override entries are trusted as-is (explicit user intent);
        // platform defaults must exist and be executable.
        let usable = if source == "env" {
            path.exists()
        } else {
            is_executable_file(&path)
        };
        if usable {
            return Ok(BrowserExecutable { path, source });
        }
    }
    Err(DetectError { searched })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn test_candidates_are_platform_shaped_and_unique() {
        let cands = candidate_paths();
        assert!(!cands.is_empty());
        let mut seen = std::collections::HashSet::new();
        for (p, source) in &cands {
            assert!(!p.as_os_str().is_empty());
            assert!(!source.is_empty());
            assert!(seen.insert(p.clone()), "duplicate candidate {p:?}");
        }
        if cfg!(target_os = "linux") {
            assert!(
                cands
                    .iter()
                    .any(|(p, _)| p == std::path::Path::new("/usr/bin/chromium"))
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_env_override_wins_and_requires_existence() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("my-chrome");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        // SAFETY: single-threaded test mutation of an env var, matching the
        // existing HomeGuard pattern in this workspace.
        unsafe {
            std::env::set_var("SHANNON_BROWSER_PATH", &fake);
        }
        let found = detect_system_browser().unwrap();
        assert_eq!(found.path, fake);
        assert_eq!(found.source, "env");

        // A non-existent override must NOT be returned — detection falls
        // through to platform candidates (which may legitimately find a real
        // browser on this machine), but never reports the missing override.
        unsafe {
            std::env::set_var("SHANNON_BROWSER_PATH", dir.path().join("missing"));
        }
        match detect_system_browser() {
            Ok(found) => {
                assert_ne!(found.source, "env");
                assert_ne!(found.path, dir.path().join("missing"));
            }
            Err(err) => assert!(!err.searched.is_empty()),
        }
        unsafe {
            std::env::remove_var("SHANNON_BROWSER_PATH");
        }
    }

    #[test]
    fn test_detect_error_display_lists_paths_and_hint() {
        let err = DetectError {
            searched: vec![PathBuf::from("/usr/bin/chromium")],
        };
        let msg = err.to_string();
        assert!(msg.contains("/usr/bin/chromium"));
        assert!(msg.contains("install"), "hint present: {msg}");
    }
}
