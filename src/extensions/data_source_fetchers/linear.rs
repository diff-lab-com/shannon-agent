//! Linear data source fetcher.
//!
//! Queries Linear issues via the GraphQL API:
//! POST https://api.linear.app/graphql with GraphQL query
//! Auth: Authorization: <api_key> (no Bearer prefix)

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Linear API fetcher.
#[derive(Debug, Clone, Copy)]
pub struct LinearFetcher;

#[async_trait]
impl DataSourceFetcher for LinearFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let api_key = config
            .get("api_key")
            .ok_or(DataSourceError::MissingConfig("api_key".into()))?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(DataSourceError::RequestError)?;

        // Build GraphQL query
        let graphql_query = self.build_query(config, query);

        let response = client
            .post("https://api.linear.app/graphql")
            .header("Authorization", api_key)
            .header("Content-Type", "application/json")
            .json(&graphql_query)
            .send()
            .await
            .map_err(DataSourceError::RequestError)?;

        self.handle_response(response).await
    }
}

impl LinearFetcher {
    fn build_query(&self, config: &BTreeMap<String, String>, query: &str) -> serde_json::Value {
        // Filter by team if configured
        let team_filter = if let Some(team_key) = config.get("team_key") {
            format!(r#"team: {{ key: {{ eq: "{}" }} }},"#, team_key)
        } else {
            String::new()
        };

        // Add search filter if query is non-empty
        let search_filter = if !query.is_empty() {
            format!(
                r#"filter: {{ {{ {} OR title: {{ containsIgnoreCase: "{}" }} OR description: {{ containsIgnoreCase: "{}" }} }} }}"#,
                team_filter.trim_end_matches(','),
                query.replace('"', "\\\""),
                query.replace('"', "\\\"")
            )
        } else if !team_filter.is_empty() {
            format!(r#"filter: {{ {} }}"#, team_filter)
        } else {
            String::new()
        };

        serde_json::json!({
            "query": format!(r#"
                query {{
                    issues({}) {{
                        nodes {{
                            id
                            title
                            description
                            url
                            updatedAt
                            state {{
                                name
                            }}
                        }}
                    }}
                }}
            "#, search_filter.trim())
        })
    }

    async fn handle_response(
        &self,
        response: reqwest::Response,
    ) -> Result<DataSourceResult, DataSourceError> {
        let status = response.status();

        if status.is_success() {
            let body: LinearResponse = response.json().await?;
            let items = body
                .data
                .issues
                .nodes
                .into_iter()
                .map(|issue| self.map_issue(issue))
                .collect();

            Ok(DataSourceResult {
                items,
                total: 0,        // Linear GraphQL doesn't return total count
                has_more: false, // Simplified — Linear uses pagination
            })
        } else {
            match status.as_u16() {
                401 | 403 => Err(DataSourceError::AuthError),
                429 => Err(DataSourceError::RateLimited),
                _ if status.is_server_error() => Err(DataSourceError::UpstreamError(format!(
                    "Linear returned {}",
                    status
                ))),
                _ => Err(DataSourceError::UpstreamError(format!(
                    "Linear returned {}",
                    status
                ))),
            }
        }
    }

    fn map_issue(&self, issue: LinearIssue) -> DataSourceItem {
        DataSourceItem {
            id: issue.id,
            title: issue.title,
            body: issue.description,
            url: Some(issue.url),
            kind: "issue".into(),
            updated_at: Some(issue.updated_at),
        }
    }
}

/// Linear GraphQL response shape.
#[derive(Debug, Deserialize)]
struct LinearResponse {
    data: LinearData,
}

/// Linear data wrapper.
#[derive(Debug, Deserialize)]
struct LinearData {
    issues: LinearIssues,
}

/// Linear issues collection.
#[derive(Debug, Deserialize)]
struct LinearIssues {
    nodes: Vec<LinearIssue>,
}

/// Linear issue object.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LinearIssue {
    id: String,
    title: String,
    #[serde(default)]
    description: Option<String>,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_fetcher_requires_api_key() {
        let fetcher = LinearFetcher;
        let config = BTreeMap::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { fetcher.fetch(&config, "test").await });

        assert!(result.is_err());
        match result {
            Err(DataSourceError::MissingConfig(field)) => {
                assert_eq!(field, "api_key");
            }
            _ => panic!("Expected MissingConfig error"),
        }
    }

    #[test]
    fn linear_response_deserializes_successfully() {
        let json = r#"{
            "data": {
                "issues": {
                    "nodes": [
                        {
                            "id": "LIN-1",
                            "title": "Test Issue",
                            "description": "Test description",
                            "url": "https://linear.app/issue/LIN-1",
                            "updatedAt": "2024-01-01T00:00:00.000Z"
                        }
                    ]
                }
            }
        }"#;

        let response: LinearResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.data.issues.nodes.len(), 1);
        assert_eq!(response.data.issues.nodes[0].id, "LIN-1");
    }

    #[test]
    fn linear_maps_issue_to_data_source_item() {
        let fetcher = LinearFetcher;

        let issue = LinearIssue {
            id: "LIN-1".into(),
            title: "Test Issue".into(),
            description: Some("Test description".into()),
            url: "https://linear.app/issue/LIN-1".into(),
            updated_at: "2024-01-01T00:00:00.000Z".into(),
        };

        let item = fetcher.map_issue(issue);
        assert_eq!(item.id, "LIN-1");
        assert_eq!(item.title, "Test Issue");
        assert_eq!(item.body, Some("Test description".into()));
        assert_eq!(item.kind, "issue");
    }
}
