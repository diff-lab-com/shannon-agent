//! Jira data source fetcher.
//!
//! Queries Jira Cloud issues via the REST API:
//! GET https://{domain}/rest/api/3/search?jql=project={project_key} ORDER BY updated DESC&maxResults=50
//! Auth: HTTP Basic with email:api_token
//! If no project_key configured, omit from JQL.

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Jira API fetcher.
#[derive(Debug, Clone, Copy)]
pub struct JiraFetcher;

#[async_trait]
impl DataSourceFetcher for JiraFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let domain = config
            .get("domain")
            .ok_or(DataSourceError::MissingConfig("domain".into()))?;
        let email = config
            .get("email")
            .ok_or(DataSourceError::MissingConfig("email".into()))?;
        let api_token = config
            .get("api_token")
            .ok_or(DataSourceError::MissingConfig("api_token".into()))?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(DataSourceError::RequestError)?;

        let url = self.build_url(domain, config, query);

        let response = client
            .get(&url)
            .header("Authorization", format!("Basic {}", self.build_basic_auth(email, api_token)))
            .header("Content-Type", "application/json")
            .send()
            .await
            .map_err(DataSourceError::RequestError)?;

        self.handle_response(response).await
    }
}

impl JiraFetcher {
    fn build_url(&self, domain: &str, config: &BTreeMap<String, String>, query: &str) -> String {
        let jql = self.build_jql(config, query);
        format!(
            "https://{}/rest/api/3/search?jql={}&maxResults=50",
            domain,
            percent_encoding::utf8_percent_encode(&jql, percent_encoding::NON_ALPHANUMERIC)
        )
    }

    fn build_jql(&self, config: &BTreeMap<String, String>, query: &str) -> String {
        let mut jql_parts = Vec::new();

        // Add project filter if configured
        if let Some(project_key) = config.get("project_key") {
            jql_parts.push(format!("project = {}", project_key));
        }

        // Add text search if query provided
        if !query.is_empty() {
            jql_parts.push(format!(r#"text ~ "{}""#, query.replace('"', "\\\"")));
        }

        // Always order by updated date
        let jql_filter = if jql_parts.is_empty() {
            String::new()
        } else {
            jql_parts.join(" AND ")
        };

        if jql_filter.is_empty() {
            "ORDER BY updated DESC".to_string()
        } else {
            format!("{} ORDER BY updated DESC", jql_filter)
        }
    }

    fn build_basic_auth(&self, email: &str, api_token: &str) -> String {
        use base64::Engine;
        let credentials = format!("{}:{}", email, api_token);
        base64::engine::general_purpose::STANDARD.encode(&credentials)
    }

    async fn handle_response(
        &self,
        response: reqwest::Response,
    ) -> Result<DataSourceResult, DataSourceError> {
        let status = response.status();

        if status.is_success() {
            let body: JiraResponse = response.json().await?;
            let total_count = body.total;
            let items_count = body.issues.len();
            let start_at = body.start_at;

            let items = body
                .issues
                .into_iter()
                .map(|issue| self.map_issue(issue))
                .collect();

            Ok(DataSourceResult {
                items,
                total: total_count,
                has_more: start_at + items_count < total_count,
            })
        } else {
            match status.as_u16() {
                401 | 403 => Err(DataSourceError::AuthError),
                429 => Err(DataSourceError::RateLimited),
                _ if status.is_server_error() => Err(DataSourceError::UpstreamError(format!(
                    "Jira returned {}",
                    status
                ))),
                _ => Err(DataSourceError::UpstreamError(format!(
                    "Jira returned {}",
                    status
                ))),
            }
        }
    }

    fn map_issue(&self, issue: JiraIssue) -> DataSourceItem {
        DataSourceItem {
            id: issue.id,
            title: format!("{}: {}", issue.key, issue.fields.summary),
            body: issue.fields.description,
            url: Some(format!("{}/browse/{}", issue.self_url, issue.key)),
            kind: "issue".into(),
            updated_at: Some(issue.fields.updated),
        }
    }
}

/// Jira API response shape.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JiraResponse {
    #[serde(rename = "startAt")]
    start_at: usize,
    total: usize,
    issues: Vec<JiraIssue>,
}

/// Jira issue object.
#[derive(Debug, Deserialize)]
struct JiraIssue {
    id: String,
    key: String,
    #[serde(rename = "self")]
    self_url: String,
    fields: JiraFields,
}

/// Jira issue fields.
#[derive(Debug, Deserialize)]
struct JiraFields {
    summary: String,
    #[serde(default)]
    description: Option<String>,
    updated: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jira_fetcher_requires_domain_email_and_token() {
        let fetcher = JiraFetcher;
        let config = BTreeMap::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { fetcher.fetch(&config, "test").await });

        assert!(result.is_err());
        match result {
            Err(DataSourceError::MissingConfig(field)) => {
                assert_eq!(field, "domain");
            }
            _ => panic!("Expected MissingConfig error"),
        }
    }

    #[test]
    fn jira_response_deserializes_successfully() {
        let json = r#"{
            "startAt": 0,
            "maxResults": 50,
            "total": 1,
            "issues": [
                {
                    "id": "issue-1",
                    "key": "PROJ-1",
                    "self": "https://test.atlassian.net",
                    "fields": {
                        "summary": "Test Issue",
                        "description": "Test description",
                        "updated": "2024-01-01T00:00:00.000Z"
                    }
                }
            ]
        }"#;

        let response: JiraResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.total, 1);
        assert_eq!(response.issues.len(), 1);
        assert_eq!(response.issues[0].key, "PROJ-1");
    }

    #[test]
    fn jira_maps_issue_to_data_source_item() {
        let fetcher = JiraFetcher;

        let issue = JiraIssue {
            id: "issue-1".into(),
            key: "PROJ-1".into(),
            self_url: "https://test.atlassian.net".into(),
            fields: JiraFields {
                summary: "Test Issue".into(),
                description: Some("Test description".into()),
                updated: "2024-01-01T00:00:00.000Z".into(),
            },
        };

        let item = fetcher.map_issue(issue);
        assert_eq!(item.id, "issue-1");
        assert!(item.title.contains("PROJ-1"));
        assert!(item.title.contains("Test Issue"));
        assert_eq!(item.kind, "issue");
    }
}
