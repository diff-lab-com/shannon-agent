//! GitHub Issues data source fetcher.
//!
//! Queries issues/PRs via the GitHub REST API:
//! GET https://api.github.com/repos/{owner}/{repo}/issues?state=open&per_page=50
//! If no default_repo, hit /user/issues instead.
//! Auth: Bearer <token> + Accept: application/vnd.github+json + User-Agent: shannon-desktop

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// GitHub API fetcher.
#[derive(Debug, Clone, Copy)]
pub struct GitHubFetcher;

#[async_trait]
impl DataSourceFetcher for GitHubFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let token = config
            .get("token")
            .ok_or(DataSourceError::MissingConfig("token".into()))?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(DataSourceError::RequestError)?;

        let url = self.build_url(config, query);

        let mut request = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "shannon-desktop");

        // Add query as q parameter if provided (GitHub search syntax)
        if !query.is_empty() {
            request = request.query(&[("q", query)]);
        }

        let response = request
            .send()
            .await
            .map_err(DataSourceError::RequestError)?;

        self.handle_response(response).await
    }
}

impl GitHubFetcher {
    fn build_url(&self, config: &BTreeMap<String, String>, query: &str) -> String {
        if let Some(default_repo) = config.get("default_repo") {
            // Query specific repository
            if query.is_empty() {
                // List open issues
                format!(
                    "https://api.github.com/repos/{}/issues?state=open&per_page=50",
                    default_repo
                )
            } else {
                // Search within repository
                format!(
                    "https://api.github.com/search/issues?q=repo:{}+{}&per_page=50",
                    default_repo,
                    percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
                )
            }
        } else {
            // Query all user's issues
            if query.is_empty() {
                "https://api.github.com/user/issues?state=open&per_page=50".to_string()
            } else {
                format!(
                    "https://api.github.com/search/issues?q={}&per_page=50",
                    percent_encoding::utf8_percent_encode(query, percent_encoding::NON_ALPHANUMERIC)
                )
            }
        }
    }

    async fn handle_response(
        &self,
        response: reqwest::Response,
    ) -> Result<DataSourceResult, DataSourceError> {
        let status = response.status();

        if status.is_success() {
            // Check if this is a search response (has "items" array) or list response (direct array)
            let text = response.text().await?;

            if let Ok(search_response) = serde_json::from_str::<GitHubSearchResponse>(&text) {
                // Search response
                let items = search_response
                    .items
                    .into_iter()
                    .map(|issue| self.map_issue(issue))
                    .collect();
                let total = search_response.total_count;

                Ok(DataSourceResult {
                    items,
                    total,
                    has_more: false,
                })
            } else if let Ok(issues) = serde_json::from_str::<Vec<GitHubIssue>>(&text) {
                // List response
                let items: Vec<DataSourceItem> = issues.into_iter().map(|issue| self.map_issue(issue)).collect();
                let total = items.len();

                Ok(DataSourceResult {
                    items,
                    total,
                    has_more: false,
                })
            } else {
                Err(DataSourceError::UpstreamError("Failed to parse GitHub response".to_string()))
            }
        } else {
            match status.as_u16() {
                401 | 403 => Err(DataSourceError::AuthError),
                429 => Err(DataSourceError::RateLimited),
                _ if status.is_server_error() => Err(DataSourceError::UpstreamError(format!(
                    "GitHub returned {}",
                    status
                ))),
                _ => Err(DataSourceError::UpstreamError(format!(
                    "GitHub returned {}",
                    status
                ))),
            }
        }
    }

    fn map_issue(&self, issue: GitHubIssue) -> DataSourceItem {
        let kind = if issue.pull_request.is_some() {
            "pr"
        } else {
            "issue"
        };

        DataSourceItem {
            id: issue.node_id,
            title: issue.title,
            body: issue.body,
            url: Some(issue.html_url),
            kind: kind.to_string(),
            updated_at: Some(issue.updated_at),
        }
    }
}

/// GitHub search response (for search queries).
#[derive(Debug, Deserialize)]
struct GitHubSearchResponse {
    total_count: usize,
    items: Vec<GitHubIssue>,
}

/// GitHub issue/PR object.
#[derive(Debug, Deserialize)]
struct GitHubIssue {
    node_id: String,
    title: String,
    #[serde(default)]
    body: Option<String>,
    html_url: String,
    updated_at: String,
    #[serde(default)]
    pull_request: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_fetcher_requires_token() {
        let fetcher = GitHubFetcher;
        let config = BTreeMap::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { fetcher.fetch(&config, "test").await });

        assert!(result.is_err());
        match result {
            Err(DataSourceError::MissingConfig(field)) => {
                assert_eq!(field, "token");
            }
            _ => panic!("Expected MissingConfig error"),
        }
    }

    #[test]
    fn github_search_response_deserializes_successfully() {
        let json = r#"{
            "total_count": 1,
            "items": [
                {
                    "node_id": "issue-1",
                    "title": "Test Issue",
                    "body": "Test description",
                    "html_url": "https://github.com/test/repo/issues/1",
                    "updated_at": "2024-01-01T00:00:00Z",
                    "pull_request": null
                }
            ]
        }"#;

        let response: GitHubSearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total_count, 1);
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].node_id, "issue-1");
    }

    #[test]
    fn github_issue_list_deserializes_successfully() {
        let json = r#"[
            {
                "node_id": "issue-1",
                "title": "Test Issue",
                "body": "Test description",
                "html_url": "https://github.com/test/repo/issues/1",
                "updated_at": "2024-01-01T00:00:00Z",
                "pull_request": null
            }
        ]"#;

        let issues: Vec<GitHubIssue> = serde_json::from_str(json).unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].node_id, "issue-1");
    }

    #[test]
    fn github_distinguishes_pr_from_issue() {
        let fetcher = GitHubFetcher;

        let issue = GitHubIssue {
            node_id: "node-1".into(),
            title: "Test".into(),
            body: Some("Body".into()),
            html_url: "https://github.com/test/repo/pull/1".into(),
            updated_at: "2024-01-01T00:00:00Z".into(),
            pull_request: Some(serde_json::json!({})),
        };

        let item = fetcher.map_issue(issue);
        assert_eq!(item.kind, "pr");

        let issue2 = GitHubIssue {
            node_id: "node-2".into(),
            title: "Test".into(),
            body: Some("Body".into()),
            html_url: "https://github.com/test/repo/issues/1".into(),
            updated_at: "2024-01-01T00:00:00Z".into(),
            pull_request: None,
        };

        let item2 = fetcher.map_issue(issue2);
        assert_eq!(item2.kind, "issue");
    }
}
