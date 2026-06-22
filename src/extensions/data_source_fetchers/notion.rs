//! Notion data source fetcher.
//!
//! Queries Notion pages/databases via the REST API:
//! - POST https://api.notion.com/v1/databases/{database_id}/query
//! - GET https://api.notion.com/v1/pages/{id} (fallback if no database configured)
//! Auth: Bearer <integration_token> + Notion-Version: 2022-06-28

use super::{DataSourceError, DataSourceFetcher, DataSourceItem, DataSourceResult};
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Notion API fetcher.
#[derive(Debug, Clone, Copy)]
pub struct NotionFetcher;

#[async_trait]
impl DataSourceFetcher for NotionFetcher {
    async fn fetch(
        &self,
        config: &BTreeMap<String, String>,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let token = config
            .get("integration_token")
            .ok_or(DataSourceError::MissingConfig("integration_token".into()))?;

        let client = reqwest::Client::builder()
            .build()
            .map_err(DataSourceError::RequestError)?;

        // If database_id is configured, query the database; otherwise search all pages
        if let Some(database_id) = config.get("database_id") {
            self.query_database(&client, token, database_id, query)
                .await
        } else {
            self.search_pages(&client, token, query).await
        }
    }
}

impl NotionFetcher {
    async fn query_database(
        &self,
        client: &reqwest::Client,
        token: &str,
        database_id: &str,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let url = format!("https://api.notion.com/v1/databases/{}/query", database_id);

        let mut request_body = serde_json::json!({
            "page_size": 50
        });

        // Add text filter if query is non-empty
        if !query.is_empty() {
            request_body["filter"] = serde_json::json!({
                "property": "title",
                "title": {
                    "contains": query
                }
            });
        }

        let response = client
            .post(&*url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Notion-Version", "2022-06-28")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(DataSourceError::RequestError)?;

        self.handle_response(response).await
    }

    async fn search_pages(
        &self,
        client: &reqwest::Client,
        token: &str,
        query: &str,
    ) -> Result<DataSourceResult, DataSourceError> {
        let url = "https://api.notion.com/v1/search";

        let mut request_body = serde_json::json!({
            "page_size": 50
        });

        if !query.is_empty() {
            request_body["filter"] = serde_json::json!({
                "value": "page",
                "property": "object"
            });
            request_body["query"] = serde_json::json!(query);
        }

        let response = client
            .post(&*url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Notion-Version", "2022-06-28")
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(DataSourceError::RequestError)?;

        self.handle_response(response).await
    }

    async fn handle_response(
        &self,
        response: reqwest::Response,
    ) -> Result<DataSourceResult, DataSourceError> {
        let status = response.status();

        if status.is_success() {
            let body: NotionResponse = response.json().await?;
            let items = body
                .results
                .into_iter()
                .filter_map(|page| self.map_page(page))
                .collect();

            Ok(DataSourceResult {
                items,
                total: 0, // Notion API doesn't return total count
                has_more: body.has_more,
            })
        } else {
            match status.as_u16() {
                401 | 403 => Err(DataSourceError::AuthError),
                429 => Err(DataSourceError::RateLimited),
                _ if status.is_server_error() => Err(DataSourceError::UpstreamError(format!(
                    "Notion returned {}",
                    status
                ))),
                _ => Err(DataSourceError::UpstreamError(format!(
                    "Notion returned {}",
                    status
                ))),
            }
        }
    }

    fn map_page(&self, page: NotionPage) -> Option<DataSourceItem> {
        let title = self.extract_title(&page)?;
        let body = self.extract_body(&page);

        Some(DataSourceItem {
            id: page.id,
            title,
            body,
            url: Some(page.url),
            kind: "page".into(),
            updated_at: page.last_edited_time,
        })
    }

    fn extract_title(&self, page: &NotionPage) -> Option<String> {
        // Try title property first
        for (_key, prop) in &page.properties {
            if let Some(prop_obj) = prop.as_object() {
                if prop_obj.get("type")?.as_str()? == "title" {
                    if let Some(title_array) = prop_obj.get("title")?.as_array() {
                        for title_part in title_array {
                            if let Some(title_obj) = title_part.as_object() {
                                if let Some(text_obj) = title_obj.get("text")?.as_object() {
                                    if let Some(text) = text_obj.get("content")?.as_str() {
                                        return Some(text.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Fallback to first plain_text property
        for (_key, prop) in &page.properties {
            if let Some(value) = self.extract_plain_text(prop) {
                if !value.is_empty() {
                    return Some(value);
                }
            }
        }

        None
    }

    fn extract_body(&self, page: &NotionPage) -> Option<String> {
        // Collect all text content from properties
        let mut body_parts = Vec::new();

        for (_key, prop) in &page.properties {
            if let Some(text) = self.extract_plain_text(prop) {
                if !text.is_empty() {
                    body_parts.push(text);
                }
            }
        }

        if body_parts.is_empty() {
            None
        } else {
            Some(body_parts.join("\n"))
        }
    }

    fn extract_plain_text(&self, prop: &serde_json::Value) -> Option<String> {
        let prop_obj = prop.as_object()?;

        // Rich text type
        if prop_obj.get("type")?.as_str()? == "rich_text" {
            if let Some(rich_text_array) = prop_obj.get("rich_text")?.as_array() {
                let texts: Vec<&str> = rich_text_array
                    .iter()
                    .filter_map(|rt| {
                        rt.as_object()?
                            .get("text")?
                            .as_object()?
                            .get("content")?
                            .as_str()
                    })
                    .collect();
                if !texts.is_empty() {
                    return Some(texts.join(""));
                }
            }
        }

        // Number type
        if prop_obj.get("type")?.as_str()? == "number" {
            if let Some(num) = prop_obj.get("number") {
                return Some(format!("{}", num.as_f64().unwrap_or(0.0)));
            }
        }

        // Select type
        if prop_obj.get("type")?.as_str()? == "select" {
            if let Some(select) = prop_obj.get("select")?.as_object() {
                if let Some(name) = select.get("name")?.as_str() {
                    return Some(name.to_string());
                }
            }
        }

        None
    }
}

/// Notion API response shape (simplified).
#[derive(Debug, Deserialize)]
struct NotionResponse {
    results: Vec<NotionPage>,
    has_more: bool,
}

/// Notion page object (simplified).
#[derive(Debug, Deserialize)]
struct NotionPage {
    id: String,
    url: String,
    #[serde(default)]
    last_edited_time: Option<String>,
    properties: std::collections::BTreeMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notion_fetcher_requires_integration_token() {
        let fetcher = NotionFetcher;
        let config = BTreeMap::new();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async { fetcher.fetch(&config, "test").await });

        assert!(result.is_err());
        match result {
            Err(DataSourceError::MissingConfig(field)) => {
                assert_eq!(field, "integration_token");
            }
            _ => panic!("Expected MissingConfig error"),
        }
    }

    #[test]
    fn notion_response_deserializes_successfully() {
        let json = r#"{
            "results": [
                {
                    "id": "page-1",
                    "url": "https://notion.so/page-1",
                    "last_edited_time": "2024-01-01T00:00:00.000Z",
                    "properties": {}
                }
            ],
            "has_more": false
        }"#;

        let response: NotionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.results.len(), 1);
        assert_eq!(response.results[0].id, "page-1");
        assert!(!response.has_more);
    }

    #[test]
    fn notion_page_extracts_title_from_property() {
        let fetcher = NotionFetcher;

        let json = r#"{
            "id": "page-1",
            "url": "https://notion.so/page-1",
            "last_edited_time": "2024-01-01T00:00:00.000Z",
            "properties": {
                "Name": {
                    "type": "title",
                    "title": [{"text": {"content": "Test Page"}}]
                }
            }
        }"#;

        let page: NotionPage = serde_json::from_str(json).unwrap();
        let title = fetcher.extract_title(&page);
        assert_eq!(title, Some("Test Page".to_string()));
    }
}
