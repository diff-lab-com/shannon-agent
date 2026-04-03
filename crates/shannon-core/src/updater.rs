//! # Auto-Updater
//!
//! Checks for new releases of Shannon Code and notifies the user when updates
//! are available. Integrates with the GitHub Releases API to compare semantic
//! versions.
//!
//! ## Example
//!
//! ```no_run
//! use shannon_core::updater::{AutoUpdater, UpdaterConfig};
//! use std::time::Duration;
//!
//! # #[tokio::main]
//! # async fn main() {
//! let config = UpdaterConfig {
//!     repo: "shannon-code/shannon".to_string(),
//!     check_interval: Duration::from_secs(86400),
//!     enabled: true,
//!     include_prereleases: false,
//! };
//!
//! let mut updater = AutoUpdater::new(config);
//! let status = updater.check_for_update().await;
//! match status {
//!     shannon_core::updater::UpdateStatus::UpdateAvailable { latest, .. } => {
//!         println!("Update available: {}", latest);
//!     }
//!     shannon_core::updater::UpdateStatus::UpToDate { .. } => {
//!         println!("Already on latest version");
//!     }
//!     shannon_core::updater::UpdateStatus::CheckFailed { error } => {
//!         eprintln!("Failed to check for updates: {}", error);
//!     }
//! }
//! # }
//! ```

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Current version, read from Cargo.toml at compile time.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Errors that can occur during update operations.
#[derive(Error, Debug)]
pub enum UpdateError {
    /// Network request to the GitHub API failed.
    #[error("Network error checking for updates: {0}")]
    Network(#[from] reqwest::Error),

    /// The GitHub API returned a non-success status code.
    #[error("GitHub API returned status {status}: {body}")]
    ApiError { status: u16, body: String },

    /// Rate limited by the GitHub API.
    #[error("Rate limited by GitHub API. Retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    /// Could not parse a version string.
    #[error("Invalid version string: {0}")]
    InvalidVersion(String),

    /// No releases found for the repository.
    #[error("No releases found for repository '{repo}'")]
    NoReleases { repo: String },
}

/// Information about a GitHub release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// Git tag name (e.g. "v0.2.0").
    pub tag_name: String,
    /// Human-readable release name.
    pub name: Option<String>,
    /// Release body / changelog.
    pub body: Option<String>,
    /// ISO 8601 publish timestamp.
    pub published_at: String,
    /// URL to the release page on GitHub.
    pub html_url: String,
    /// Whether this is a pre-release.
    pub prerelease: bool,
}

/// Result of an update check.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// The running version is already the latest.
    UpToDate {
        current: String,
    },
    /// A newer version is available.
    UpdateAvailable {
        current: String,
        latest: String,
        release: ReleaseInfo,
    },
    /// The check itself failed (network, API, etc.).
    CheckFailed {
        error: String,
    },
}

/// Configuration for the auto-updater.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdaterConfig {
    /// GitHub `owner/repo` used to query the releases API.
    pub repo: String,
    /// Minimum elapsed time between automatic checks.
    #[serde(
        serialize_with = "serialize_duration_secs",
        deserialize_with = "deserialize_duration_secs"
    )]
    pub check_interval: Duration,
    /// Master toggle for the updater.
    pub enabled: bool,
    /// Whether to consider pre-releases as valid updates.
    pub include_prereleases: bool,
}

fn serialize_duration_secs<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.serialize_u64(d.as_secs())
}

fn deserialize_duration_secs<'de, D>(d: D) -> Result<Duration, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let secs = u64::deserialize(d)?;
    Ok(Duration::from_secs(secs))
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            repo: "shannon-code/shannon".to_string(),
            check_interval: Duration::from_secs(86400), // 24 hours
            enabled: true,
            include_prereleases: false,
        }
    }
}

/// The auto-updater: periodically checks GitHub Releases for a newer version.
pub struct AutoUpdater {
    config: UpdaterConfig,
    client: reqwest::Client,
    last_check: Instant,
    cached_status: Option<UpdateStatus>,
}

impl AutoUpdater {
    /// Create a new updater with the given configuration.
    pub fn new(config: UpdaterConfig) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(format!("shannon-code/{}", CURRENT_VERSION))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");

        Self {
            config,
            client,
            last_check: Instant::now() - Duration::from_secs(u64::MAX), // force first check
            cached_status: None,
        }
    }

    /// Check for updates, respecting `check_interval`. If the interval has not
    /// elapsed since the last check the cached result is returned instead.
    pub async fn check_for_update(&mut self) -> UpdateStatus {
        if !self.config.enabled {
            return UpdateStatus::UpToDate {
                current: CURRENT_VERSION.to_string(),
            };
        }

        if self.last_check.elapsed() < self.config.check_interval {
            debug!("update check skipped; interval not elapsed");
            return self
                .cached_status
                .clone()
                .unwrap_or(UpdateStatus::UpToDate {
                    current: CURRENT_VERSION.to_string(),
                });
        }

        self.force_check().await
    }

    /// Check for updates regardless of when the last check was performed.
    pub async fn force_check(&mut self) -> UpdateStatus {
        self.last_check = Instant::now();

        let result = match self.fetch_latest_release().await {
            Ok(release) => {
                let latest_tag = release.tag_name.clone();
                let latest_clean = Self::strip_version_prefix(&latest_tag);

                match Self::compare_versions(CURRENT_VERSION, &latest_clean) {
                    Ordering::Less => {
                        info!(
                            "update available: {} -> {}",
                            CURRENT_VERSION, latest_tag
                        );
                        UpdateStatus::UpdateAvailable {
                            current: CURRENT_VERSION.to_string(),
                            latest: latest_tag,
                            release,
                        }
                    }
                    _ => UpdateStatus::UpToDate {
                        current: CURRENT_VERSION.to_string(),
                    },
                }
            }
            Err(e) => {
                warn!("update check failed: {}", e);
                UpdateStatus::CheckFailed {
                    error: e.to_string(),
                }
            }
        };

        self.cached_status = Some(result.clone());
        result
    }

    /// Return the most recently cached status without performing a check.
    pub fn cached_status(&self) -> Option<&UpdateStatus> {
        self.cached_status.as_ref()
    }

    /// Fetch the latest (or latest non-prerelease) release from the GitHub API.
    async fn fetch_latest_release(&self) -> Result<ReleaseInfo, UpdateError> {
        let url = format!(
            "https://api.github.com/repos/{}/releases/latest",
            self.config.repo
        );

        debug!("checking releases at {}", url);

        let response = self.client.get(&url).send().await?;

        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        // Handle rate limiting
        if status.as_u16() == 403 && body.contains("rate limit exceeded") {
            let retry_after = self
                .extract_retry_after(&body)
                .unwrap_or(3600);
            return Err(UpdateError::RateLimited {
                retry_after_secs: retry_after,
            });
        }

        if !status.is_success() {
            return Err(UpdateError::ApiError {
                status: status.as_u16(),
                body: truncate_string(&body, 256),
            });
        }

        let release: ReleaseInfo = serde_json::from_str(&body).map_err(|e| {
            UpdateError::InvalidVersion(format!("failed to parse release JSON: {}", e))
        })?;

        // Skip pre-releases when not requested
        if release.prerelease && !self.config.include_prereleases {
            return self.fetch_latest_stable_release().await;
        }

        Ok(release)
    }

    /// Iterate over the releases list to find the latest non-prerelease.
    async fn fetch_latest_stable_release(&self) -> Result<ReleaseInfo, UpdateError> {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=30",
            self.config.repo
        );

        let response = self.client.get(&url).send().await?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(UpdateError::ApiError {
                status: status.as_u16(),
                body: truncate_string(&body, 256),
            });
        }

        let releases: Vec<ReleaseInfo> = serde_json::from_str(&body).map_err(|e| {
            UpdateError::InvalidVersion(format!("failed to parse releases JSON: {}", e))
        })?;

        releases
            .into_iter()
            .find(|r| !r.prerelease)
            .ok_or_else(|| UpdateError::NoReleases {
                repo: self.config.repo.clone(),
            })
    }

    /// Try to extract a `retry-after` value from the response body.
    fn extract_retry_after(&self, body: &str) -> Option<u64> {
        // GitHub rate-limit responses include a "message" field with the reset time.
        // We fall back to a generous default if parsing fails.
        body.lines()
            .find(|l| l.contains("rate limit"))
            .and_then(|line| {
                // Look for a Unix-timestamp style hint
                line.split("reset")
                    .nth(1)?
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<u64>().ok())
            })
    }

    /// Compare two semantic version strings.
    ///
    /// Returns `Ordering::Less` when `current < latest`, i.e. an update is
    /// available.
    pub fn compare_versions(current: &str, latest: &str) -> Ordering {
        let cur = Self::parse_version(current);
        let lat = Self::parse_version(latest);

        match (cur, lat) {
            (Some(c), Some(l)) => c.cmp(&l),
            // Fall back to lexicographic comparison when parsing fails
            _ => current.cmp(latest),
        }
    }

    /// Parse a version string into a `(major, minor, patch)` triple.
    ///
    /// Handles formats: `"1.2.3"`, `"v1.2.3"`, `"1.2.3-pre"`.
    pub fn parse_version(version: &str) -> Option<(u32, u32, u32)> {
        let stripped = Self::strip_version_prefix(version);

        let mut parts = stripped.splitn(4, |c: char| !c.is_ascii_digit());
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        let patch: u32 = parts.next()?.parse().ok()?;

        Some((major, minor, patch))
    }

    /// Strip a leading `v` or `V` from a version tag.
    fn strip_version_prefix(version: &str) -> &str {
        version.strip_prefix('v').or_else(|| version.strip_prefix('V')).unwrap_or(version)
    }

    /// Human-readable summary suitable for printing to the terminal.
    pub fn format_update_message(status: &UpdateStatus) -> Option<String> {
        match status {
            UpdateStatus::UpdateAvailable {
                current,
                latest,
                release,
            } => {
                let mut msg = format!(
                    "A new version of Shannon Code is available: {} -> {}\n",
                    current, latest
                );
                if let Some(ref name) = release.name {
                    msg.push_str(&format!("  Release: {}\n", name));
                }
                msg.push_str(&format!("  {}\n", release.html_url));
                Some(msg)
            }
            _ => None,
        }
    }
}

/// Truncate a string to `max_len` characters, appending "..." if truncated.
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // -- Version parsing ----------------------------------------------------

    #[test]
    fn parse_standard_version() {
        assert_eq!(AutoUpdater::parse_version("1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn parse_version_with_v_prefix() {
        assert_eq!(AutoUpdater::parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(AutoUpdater::parse_version("V2.0.0"), Some((2, 0, 0)));
    }

    #[test]
    fn parse_version_with_pre_suffix() {
        assert_eq!(AutoUpdater::parse_version("1.2.3-alpha.1"), Some((1, 2, 3)));
        assert_eq!(AutoUpdater::parse_version("v0.1.0-rc.2"), Some((0, 1, 0)));
    }

    #[test]
    fn parse_version_missing_patch() {
        // Only two parts -- no patch segment means parsing fails
        assert_eq!(AutoUpdater::parse_version("1.2"), None);
    }

    #[test]
    fn parse_version_empty() {
        assert_eq!(AutoUpdater::parse_version(""), None);
    }

    #[test]
    fn parse_version_non_numeric() {
        assert_eq!(AutoUpdater::parse_version("abc"), None);
    }

    // -- Version comparison --------------------------------------------------

    #[test]
    fn compare_equal_versions() {
        assert_eq!(
            AutoUpdater::compare_versions("1.2.3", "1.2.3"),
            Ordering::Equal
        );
    }

    #[test]
    fn compare_older_current() {
        assert_eq!(
            AutoUpdater::compare_versions("1.0.0", "2.0.0"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_newer_current() {
        assert_eq!(
            AutoUpdater::compare_versions("2.0.0", "1.0.0"),
            Ordering::Greater
        );
    }

    #[test]
    fn compare_patch_difference() {
        assert_eq!(
            AutoUpdater::compare_versions("1.2.3", "1.2.4"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_with_v_prefix() {
        assert_eq!(
            AutoUpdater::compare_versions("v1.0.0", "v2.0.0"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_mixed_prefix() {
        assert_eq!(
            AutoUpdater::compare_versions("1.0.0", "v1.0.1"),
            Ordering::Less
        );
    }

    #[test]
    fn compare_invalid_falls_back_to_lexicographic() {
        // Neither parses as semver, so we get lexicographic order
        assert_eq!(
            AutoUpdater::compare_versions("abc", "def"),
            Ordering::Less
        );
    }

    // -- Config defaults and serialization -----------------------------------

    #[test]
    fn default_config_values() {
        let config = UpdaterConfig::default();
        assert_eq!(config.repo, "shannon-code/shannon");
        assert_eq!(config.check_interval, Duration::from_secs(86400));
        assert!(config.enabled);
        assert!(!config.include_prereleases);
    }

    #[test]
    fn config_roundtrip_serialization() {
        let config = UpdaterConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: UpdaterConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.repo, config.repo);
        assert_eq!(deserialized.check_interval, config.check_interval);
        assert_eq!(deserialized.enabled, config.enabled);
        assert_eq!(
            deserialized.include_prereleases,
            config.include_prereleases
        );
    }

    #[test]
    fn config_custom_values() {
        let config = UpdaterConfig {
            repo: "myorg/myrepo".to_string(),
            check_interval: Duration::from_secs(3600),
            enabled: false,
            include_prereleases: true,
        };
        assert_eq!(config.repo, "myorg/myrepo");
        assert_eq!(config.check_interval, Duration::from_secs(3600));
        assert!(!config.enabled);
        assert!(config.include_prereleases);
    }

    // -- UpdateStatus display ------------------------------------------------

    #[test]
    fn format_update_message_available() {
        let status = UpdateStatus::UpdateAvailable {
            current: "0.1.0".to_string(),
            latest: "0.2.0".to_string(),
            release: ReleaseInfo {
                tag_name: "v0.2.0".to_string(),
                name: Some("Shannon v0.2.0".to_string()),
                body: Some("Bug fixes".to_string()),
                published_at: "2026-01-01T00:00:00Z".to_string(),
                html_url: "https://github.com/shannon-code/shannon/releases/tag/v0.2.0"
                    .to_string(),
                prerelease: false,
            },
        };
        let msg = AutoUpdater::format_update_message(&status).unwrap();
        assert!(msg.contains("0.1.0"));
        assert!(msg.contains("0.2.0"));
        assert!(msg.contains("Shannon v0.2.0"));
        assert!(msg.contains("github.com"));
    }

    #[test]
    fn format_update_message_up_to_date() {
        let status = UpdateStatus::UpToDate {
            current: "0.1.0".to_string(),
        };
        assert!(AutoUpdater::format_update_message(&status).is_none());
    }

    #[test]
    fn format_update_message_failed() {
        let status = UpdateStatus::CheckFailed {
            error: "network error".to_string(),
        };
        assert!(AutoUpdater::format_update_message(&status).is_none());
    }

    // -- ReleaseInfo serialization ------------------------------------------

    #[test]
    fn release_info_serialization() {
        let release = ReleaseInfo {
            tag_name: "v1.0.0".to_string(),
            name: Some("First release".to_string()),
            body: Some("Initial release".to_string()),
            published_at: "2026-01-01T00:00:00Z".to_string(),
            html_url: "https://github.com/test/repo".to_string(),
            prerelease: false,
        };
        let json = serde_json::to_string(&release).unwrap();
        let deserialized: ReleaseInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tag_name, "v1.0.0");
        assert_eq!(deserialized.name, Some("First release".to_string()));
        assert!(!deserialized.prerelease);
    }

    // -- strip_version_prefix helper ----------------------------------------

    #[test]
    fn strip_prefix() {
        assert_eq!(AutoUpdater::strip_version_prefix("v1.0.0"), "1.0.0");
        assert_eq!(AutoUpdater::strip_version_prefix("V1.0.0"), "1.0.0");
        assert_eq!(AutoUpdater::strip_version_prefix("1.0.0"), "1.0.0");
    }

    // -- truncate_string helper ---------------------------------------------

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let result = truncate_string("abcdefghij", 5);
        assert_eq!(result, "abcde...");
    }

    // -- Current version constant -------------------------------------------

    #[test]
    fn current_version_is_set() {
        assert!(!CURRENT_VERSION.is_empty());
    }

    #[test]
    fn current_version_parses() {
        assert!(AutoUpdater::parse_version(CURRENT_VERSION).is_some());
    }
}
