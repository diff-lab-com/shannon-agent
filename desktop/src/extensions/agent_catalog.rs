//! P4 Agents federated catalog.
//!
//! Aggregates agent collections from upstreams:
//! 1. `VoltAgent/awesome-claude-code-agents` — community curated list.
//! 2. `rohitg00/claude-code-agents` — community agents.
//! 3. Shannon built-in native agents (no upstream — baked into the binary).
//!
//! Each upstream exposes a `shannon-agents.json` manifest at its root
//! describing the available agent definitions. We fetch, parse, and convert
//! them to `CatalogEntry` rows. Results are cached 24h on disk.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::catalog::HttpFetch;
use super::installer::InstallError;
use super::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};

/// Static description of an agent collection upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentUpstream {
    pub slug: String,
    pub display_name: String,
    pub repo: String,
    pub ref_: String,
    pub trust: TrustLevel,
}

/// Static list of federated agent upstreams.
pub fn agent_upstreams() -> Vec<AgentUpstream> {
    vec![
        AgentUpstream {
            slug: "voltagent-awesome".into(),
            display_name: "VoltAgent Awesome Claude Agents".into(),
            repo: "VoltAgent/awesome-claude-code-agents".into(),
            ref_: "main".into(),
            trust: TrustLevel::Community,
        },
        AgentUpstream {
            slug: "rohitg00-community".into(),
            display_name: "rohitg00 Claude Code Agents".into(),
            repo: "rohitg00/claude-code-agents".into(),
            ref_: "main".into(),
            trust: TrustLevel::Community,
        },
    ]
}

/// One agent entry as serialized in the upstream `shannon-agents.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifestEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub homepage_url: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Slash-command trigger, e.g. `/review`.
    #[serde(default)]
    pub trigger: Option<String>,
    /// Recommended model id.
    #[serde(default)]
    pub model: Option<String>,
    /// Tools the agent should have access to.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Full system prompt body.
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Top-level manifest schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentManifest {
    pub agents: Vec<AgentManifestEntry>,
}

/// Client that aggregates agent catalogs from federated upstreams.
pub struct AgentCatalogClient {
    http: Arc<dyn HttpFetch>,
    cache_dir: Option<PathBuf>,
    cache_ttl: Duration,
}

impl AgentCatalogClient {
    pub fn new(http: Arc<dyn HttpFetch>) -> Self {
        Self {
            http,
            cache_dir: default_agent_cache_dir(),
            cache_ttl: Duration::from_secs(24 * 60 * 60),
        }
    }

    pub fn with_cache_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(dir.into());
        self
    }

    pub fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Fetch all agents from every upstream.
    pub async fn list_agents(&self) -> Result<Vec<CatalogEntry>, InstallError> {
        let upstreams = agent_upstreams();
        let mut entries = Vec::new();

        entries.extend(builtin_agents());

        for upstream in &upstreams {
            match self.fetch_upstream(upstream).await {
                Ok(mut rows) => entries.append(&mut rows),
                Err(err) => {
                    tracing::warn!(
                        upstream = %upstream.slug,
                        repo = %upstream.repo,
                        error = %err,
                        "agent upstream fetch failed; skipping"
                    );
                }
            }
        }

        Ok(entries)
    }

    async fn fetch_upstream(
        &self,
        upstream: &AgentUpstream,
    ) -> Result<Vec<CatalogEntry>, InstallError> {
        let cache_key = format!("agents-{}-{}.json", upstream.slug, upstream.ref_);
        if let Some(cached) = read_cache(&self.cache_dir, &cache_key, self.cache_ttl) {
            return Ok(cached);
        }

        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/shannon-agents.json",
            upstream.repo, upstream.ref_
        );
        let body = self.http.fetch_json(&url).await?;
        let manifest: AgentManifest = serde_json::from_str(&body)
            .map_err(|e| InstallError::Format(format!("agent manifest parse: {e}")))?;

        let rows: Vec<CatalogEntry> = manifest
            .agents
            .into_iter()
            .map(|agent| manifest_to_entry(agent, upstream))
            .collect();

        write_cache(&self.cache_dir, &cache_key, &rows);
        Ok(rows)
    }
}

fn manifest_to_entry(agent: AgentManifestEntry, upstream: &AgentUpstream) -> CatalogEntry {
    let mut metadata = std::collections::HashMap::new();
    if let Some(trigger) = &agent.trigger {
        metadata.insert("trigger".to_string(), serde_json::json!(trigger));
    }
    if let Some(model) = &agent.model {
        metadata.insert("model".to_string(), serde_json::json!(model));
    }
    if !agent.tools.is_empty() {
        metadata.insert("tools".to_string(), serde_json::json!(agent.tools));
    }
    if let Some(prompt) = &agent.system_prompt {
        metadata.insert("system_prompt".to_string(), serde_json::json!(prompt));
    }
    metadata.insert("upstream".to_string(), serde_json::json!(upstream.slug));

    CatalogEntry {
        id: format!("gh:{}/{}/{}", upstream.repo, upstream.ref_, agent.name),
        kind: AddonKind::Agent,
        name: agent.name.clone(),
        description: agent.description,
        author: Some(upstream.repo.clone()),
        version: agent.version,
        homepage_url: agent
            .homepage_url
            .or_else(|| Some(format!("https://github.com/{}", upstream.repo))),
        license: agent.license,
        stars: None,
        last_updated: None,
        source: CatalogSource::GitHubRepo {
            repo: upstream.repo.clone(),
            ref_: Some(upstream.ref_.clone()),
        },
        trust: upstream.trust,
        metadata,
        tags: agent.tags,
    }
}

/// Shannon built-in agent definitions.
fn builtin_agents() -> Vec<CatalogEntry> {
    let native_preamble =
        |name: &str, description: &str, model: &str, tools: &[&str], tags: &[&str]| {
            let mut metadata = std::collections::HashMap::new();
            metadata.insert("model".to_string(), serde_json::json!(model));
            metadata.insert(
                "tools".to_string(),
                serde_json::json!(tools.iter().map(|s| s.to_string()).collect::<Vec<_>>()),
            );
            CatalogEntry {
                id: format!("native:agent-{name}"),
                kind: AddonKind::Agent,
                name: name.to_string(),
                description: description.to_string(),
                author: Some("Shannon".into()),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                homepage_url: None,
                license: Some("Apache-2.0".into()),
                stars: None,
                last_updated: None,
                source: CatalogSource::Native,
                trust: TrustLevel::Verified,
                metadata,
                tags: tags.iter().map(|s| s.to_string()).collect(),
            }
        };

    vec![
        native_preamble(
            "code-reviewer",
            "Reviews code for bugs, security issues, and best practices before merge.",
            "claude-sonnet-4-6",
            &["read", "grep", "glob"],
            &["code", "review", "native"],
        ),
        native_preamble(
            "researcher",
            "Deep research agent that gathers, synthesizes, and cites sources.",
            "claude-sonnet-4-6",
            &["web_search", "read", "write"],
            &["research", "native"],
        ),
        native_preamble(
            "planner",
            "Breaks down ambiguous tasks into structured implementation plans.",
            "claude-opus-4-7",
            &["read", "glob", "grep"],
            &["planning", "native"],
        ),
        native_preamble(
            "implementer",
            "Translates a plan into code changes: edit files, run tests, iterate to green.",
            "claude-sonnet-4-6",
            &["read", "write", "edit", "bash"],
            &["code", "implementation", "native"],
        ),
        native_preamble(
            "tester",
            "Writes and runs tests; tracks coverage and flags regressions on refactor.",
            "claude-sonnet-4-6",
            &["read", "write", "bash", "grep"],
            &["testing", "native"],
        ),
        native_preamble(
            "debugger",
            "Root-causes failures via bisect, stack traces, and state inspection.",
            "claude-sonnet-4-6",
            &["read", "bash", "grep", "glob"],
            &["debug", "native"],
        ),
        native_preamble(
            "refactorer",
            "Applies scope-safe refactors: extract, inline, rename, dead-code removal.",
            "claude-sonnet-4-6",
            &["read", "edit", "grep", "glob"],
            &["refactor", "native"],
        ),
        native_preamble(
            "doc-writer",
            "Drafts READMEs, ADRs, API docs, and CHANGELOG entries from code scans.",
            "claude-haiku-4-5-20251001",
            &["read", "write", "glob"],
            &["docs", "native"],
        ),
        native_preamble(
            "data-analyst",
            "Explores datasets, runs statistical summaries, and visualizes distributions.",
            "claude-sonnet-4-6",
            &["read", "bash", "write"],
            &["data", "analysis", "native"],
        ),
        native_preamble(
            "architect",
            "Proposes system designs with trade-off matrices and sequence diagrams.",
            "claude-opus-4-7",
            &["read", "glob", "grep"],
            &["architecture", "design", "native"],
        ),
    ]
}

fn default_agent_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("shannon").join("agents"))
}

fn read_cache(cache_dir: &Option<PathBuf>, key: &str, ttl: Duration) -> Option<Vec<CatalogEntry>> {
    let path = cache_dir.as_ref()?.join(key);
    let metadata = std::fs::metadata(&path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = modified.elapsed().ok()?;
    if age > ttl {
        return None;
    }
    let body = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&body).ok()
}

fn write_cache(cache_dir: &Option<PathBuf>, key: &str, rows: &[CatalogEntry]) {
    let Some(dir) = cache_dir else { return };
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join(key);
    if let Ok(body) = serde_json::to_string(rows) {
        let _ = std::fs::write(path, body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::catalog::StaticFetch;

    fn manifest_json() -> String {
        r#"{
            "agents": [
                {
                    "name": "upstream-doc-writer",
                    "description": "Technical doc writer.",
                    "version": "1.0.0",
                    "trigger": "/doc",
                    "model": "claude-sonnet-4-6",
                    "tools": ["read", "write"],
                    "system_prompt": "You write clear docs.",
                    "tags": ["docs"]
                },
                {
                    "name": "upstream-tester",
                    "description": "Writes tests.",
                    "tags": ["testing"]
                }
            ]
        }"#
        .to_string()
    }

    #[tokio::test]
    async fn lists_builtin_agents_without_network() {
        let client = AgentCatalogClient::new(Arc::new(StaticFetch("{}".to_string())));
        let entries = client.list_agents().await.expect("list");
        let native_count = entries
            .iter()
            .filter(|e| e.source == CatalogSource::Native)
            .count();
        assert!(
            native_count >= 10,
            "expected at least 10 native entries, got {native_count}"
        );
    }

    #[tokio::test]
    async fn fetches_upstream_on_cache_miss() {
        let tmp = tempfile::tempdir().expect("tmp");
        let client = AgentCatalogClient::new(Arc::new(StaticFetch(manifest_json())))
            .with_cache_dir(tmp.path());
        let entries = client.list_agents().await.expect("list");
        // 10 native + 2 agents × 2 upstreams = 14
        assert_eq!(entries.len(), 14);
        let doc = entries
            .iter()
            .find(|e| e.name == "upstream-doc-writer")
            .expect("upstream-doc-writer");
        assert_eq!(doc.kind, AddonKind::Agent);
        assert!(doc.id.starts_with("gh:"));
        let trigger = doc.metadata.get("trigger").and_then(|v| v.as_str());
        assert_eq!(trigger, Some("/doc"));
        let model = doc.metadata.get("model").and_then(|v| v.as_str());
        assert_eq!(model, Some("claude-sonnet-4-6"));
    }

    #[tokio::test]
    async fn serves_cache_on_second_call() {
        let tmp = tempfile::tempdir().expect("tmp");
        let http = Arc::new(StaticFetch(manifest_json()));
        let client = AgentCatalogClient::new(http).with_cache_dir(tmp.path());
        let first = client.list_agents().await.expect("list");
        let http_bad = Arc::new(StaticFetch("not json".to_string()));
        let client2 = AgentCatalogClient::new(http_bad).with_cache_dir(tmp.path());
        let second = client2.list_agents().await.expect("list");
        assert_eq!(first.len(), second.len());
        assert_eq!(first.len(), 14);
    }

    #[tokio::test]
    async fn skips_upstream_on_fetch_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let http = Arc::new(StaticFetch("not json".to_string()));
        let client = AgentCatalogClient::new(http).with_cache_dir(tmp.path());
        let entries = client.list_agents().await.expect("list");
        assert!(entries.iter().all(|e| e.source == CatalogSource::Native));
    }

    #[test]
    fn manifest_entry_parses_optional_fields() {
        let json = r#"{
            "name": "x",
            "description": "y",
            "trigger": "/x"
        }"#;
        let entry: AgentManifestEntry = serde_json::from_str(json).expect("parse");
        assert_eq!(entry.name, "x");
        assert_eq!(entry.trigger.as_deref(), Some("/x"));
        assert!(entry.system_prompt.is_none());
        assert!(entry.tools.is_empty());
    }
}
