//! D3 Data source fetchers — HTTP implementations for Notion/Linear/GitHub/Jira.
//!
//! Each fetcher implements the `DataSourceFetcher` trait, which takes a
//! config map (already loaded from ~/.shannon/data-sources/<slug>.toml) and
//! a query string, then returns normalized `DataSourceResult`. The
//! `dispatch` function routes based on the `kind` field from the TOML's
//! [data_source] section.

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod github;
pub mod jira;
pub mod linear;
pub mod notion;

/// Normalized result shape shared across all data sources.
#[derive(Debug, Serialize)]
pub struct DataSourceResult {
    pub items: Vec<DataSourceItem>,
    pub total: usize,
    pub has_more: bool,
}

/// Single normalized item from any data source.
#[derive(Debug, Serialize)]
pub struct DataSourceItem {
    /// Upstream id (Linear id, GitHub node_id, Notion page id, etc.).
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Plain text or markdown body.
    pub body: Option<String>,
    /// Direct URL to the item in the upstream UI.
    pub url: Option<String>,
    /// Item kind — "issue", "page", "pr", etc.
    pub kind: String,
    /// ISO 8601 timestamp if available.
    pub updated_at: Option<String>,
}

/// Error types for data source fetching.
#[derive(Debug, thiserror::Error)]
pub enum DataSourceError {
    #[error("Missing required config field: {0}")]
    MissingConfig(String),

    #[error("HTTP request failed: {0}")]
    RequestError(#[from] reqwest::Error),

    #[error("Authentication failed — check credentials")]
    AuthError,

    #[error("Rate limited by upstream API")]
    RateLimited,

    #[error("Upstream API error: {0}")]
    UpstreamError(String),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Unknown data source kind: {0}")]
    UnknownKind(String),
}

/// Trait abstraction for data source fetchers.
#[async_trait::async_trait]
pub trait DataSourceFetcher: Send + Sync {
    /// Fetch items from the data source.
    ///
    /// `config` contains the adapter-specific fields from the TOML [config] section.
    /// `query` is a user-supplied search string (implementation-specific interpretation).
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError>;
}

/// Dispatch to the right fetcher based on `kind`.
///
/// Adapters marked "config-only" (Slack, Discord, Telegram, RSS, iCal)
/// install and persist configuration today; their query path is stubbed
/// and returns a "coming soon" error. This lets the catalog surface them
/// without degrading the install/configure UX.
pub fn dispatch(kind: &str) -> Result<Arc<dyn DataSourceFetcher>, DataSourceError> {
    match kind {
        "notion" => Ok(Arc::new(notion::NotionFetcher)),
        "linear" => Ok(Arc::new(linear::LinearFetcher)),
        "github_issues" => Ok(Arc::new(github::GitHubFetcher)),
        "jira" => Ok(Arc::new(jira::JiraFetcher)),
        "slack" | "discord" | "telegram" | "rss" | "ical" => {
            Err(DataSourceError::UpstreamError(format!(
                "Query for '{kind}' is coming soon; configuration has been saved and will be used once the fetcher ships."
            )))
        }
        _ => Err(DataSourceError::UnknownKind(kind.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_returns_known_fetchers() {
        assert!(dispatch("notion").is_ok());
        assert!(dispatch("linear").is_ok());
        assert!(dispatch("github_issues").is_ok());
        assert!(dispatch("jira").is_ok());
    }

    #[test]
    fn dispatch_rejects_unknown_kind() {
        let result = dispatch("unknown_kind");
        assert!(result.is_err());
        if let Err(DataSourceError::UnknownKind(kind)) = result {
            assert_eq!(kind, "unknown_kind");
        } else {
            panic!("Expected UnknownKind error");
        }
    }

    #[test]
    fn data_source_result_serializes_correctly() {
        let result = DataSourceResult {
            items: vec![DataSourceItem {
                id: "test-1".into(),
                title: "Test Item".into(),
                body: Some("Test body".into()),
                url: Some("https://example.com/1".into()),
                kind: "issue".into(),
                updated_at: Some("2024-01-01T00:00:00Z".into()),
            }],
            total: 1,
            has_more: false,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains(r#""id":"test-1""#));
        assert!(json.contains(r#""title":"Test Item""#));
        assert!(json.contains(r#""total":1"#));
    }
}
