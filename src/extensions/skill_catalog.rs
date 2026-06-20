//! P3 Skills federated catalog.
//!
//! Aggregates skill collections from three upstreams:
//! 1. `anthropics/skills` — Anthropic's official skill demos.
//! 2. `obra/superpowers` — community-driven skill collection.
//! 3. Shannon built-in skills (no upstream — baked into the binary).
//!
//! Each upstream repo exposes a `shannon-skills.json` manifest at its root
//! describing the available skills. We fetch, parse, and convert them to
//! `CatalogEntry` rows for the hub UI. Results are cached 24h on disk.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::catalog::HttpFetch;
use super::installer::InstallError;
use super::types::{AddonKind, CatalogEntry, CatalogSource, TrustLevel};

/// Static description of a skill collection upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillUpstream {
    /// Catalog identifier slug, e.g. `"anthropics-official"`.
    pub slug: String,
    /// Display name shown in the UI header.
    pub display_name: String,
    /// `owner/repo` GitHub coordinates.
    pub repo: String,
    /// Branch or commit SHA to pin.
    pub ref_: String,
    /// Trust level for entries from this upstream.
    pub trust: TrustLevel,
}

/// Static list of federated skill upstreams.
///
/// The order here is the order entries appear in the UI.
pub fn skill_upstreams() -> Vec<SkillUpstream> {
    vec![
        SkillUpstream {
            slug: "anthropics-official".into(),
            display_name: "Anthropic Official Skills".into(),
            repo: "anthropics/skills".into(),
            ref_: "main".into(),
            trust: TrustLevel::Verified,
        },
        SkillUpstream {
            slug: "superpowers-community".into(),
            display_name: "Superpowers (community)".into(),
            repo: "obra/superpowers".into(),
            ref_: "main".into(),
            trust: TrustLevel::Community,
        },
    ]
}

/// One skill entry as serialized in the upstream `shannon-skills.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifestEntry {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub homepage_url: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// Path inside the repo where the SKILL.md lives.
    #[serde(default)]
    pub path: Option<String>,
    /// Slash-command trigger, e.g. `/deploy`.
    #[serde(default)]
    pub trigger: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Top-level manifest schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub skills: Vec<SkillManifestEntry>,
}

/// Client that aggregates skill catalogs from federated upstreams.
pub struct SkillCatalogClient {
    http: Arc<dyn HttpFetch>,
    cache_dir: Option<PathBuf>,
    cache_ttl: Duration,
}

impl SkillCatalogClient {
    pub fn new(http: Arc<dyn HttpFetch>) -> Self {
        Self {
            http,
            cache_dir: default_skill_cache_dir(),
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

    /// Fetch all skills from every upstream. Continues on per-upstream errors
    /// so one broken repo doesn't kill the whole catalog.
    pub async fn list_skills(&self) -> Result<Vec<CatalogEntry>, InstallError> {
        let upstreams = skill_upstreams();
        let mut entries = Vec::new();

        // Built-in native skills always come first.
        entries.extend(builtin_skills());

        for upstream in &upstreams {
            match self.fetch_upstream(upstream).await {
                Ok(mut rows) => entries.append(&mut rows),
                Err(err) => {
                    tracing::warn!(
                        upstream = %upstream.slug,
                        repo = %upstream.repo,
                        error = %err,
                        "skill upstream fetch failed; skipping"
                    );
                }
            }
        }

        Ok(entries)
    }

    async fn fetch_upstream(
        &self,
        upstream: &SkillUpstream,
    ) -> Result<Vec<CatalogEntry>, InstallError> {
        let cache_key = format!("skills-{}-{}.json", upstream.slug, upstream.ref_);
        if let Some(cached) = read_cache(&self.cache_dir, &cache_key, self.cache_ttl) {
            return Ok(cached);
        }

        let url = format!(
            "https://raw.githubusercontent.com/{}/{}/shannon-skills.json",
            upstream.repo, upstream.ref_
        );
        let body = self.http.fetch_json(&url).await?;
        let manifest: SkillManifest = serde_json::from_str(&body)
            .map_err(|e| InstallError::Format(format!("skill manifest parse: {e}")))?;

        let rows: Vec<CatalogEntry> = manifest
            .skills
            .into_iter()
            .map(|skill| manifest_to_entry(skill, upstream))
            .collect();

        write_cache(&self.cache_dir, &cache_key, &rows);
        Ok(rows)
    }
}

fn manifest_to_entry(skill: SkillManifestEntry, upstream: &SkillUpstream) -> CatalogEntry {
    let mut metadata = std::collections::HashMap::new();
    if let Some(trigger) = &skill.trigger {
        metadata.insert("trigger".to_string(), serde_json::json!(trigger));
    }
    if let Some(path) = &skill.path {
        metadata.insert(
            "repo_path".to_string(),
            serde_json::json!(format!(
                "https://github.com/{}/tree/{}/{}",
                upstream.repo, upstream.ref_, path
            )),
        );
    }
    metadata.insert("upstream".to_string(), serde_json::json!(upstream.slug));

    CatalogEntry {
        id: format!("gh:{}/{}/{}", upstream.repo, upstream.ref_, skill.name),
        kind: AddonKind::Skill,
        name: skill.name.clone(),
        description: skill.description,
        author: Some(upstream.repo.clone()),
        version: skill.version,
        homepage_url: skill
            .homepage_url
            .or_else(|| Some(format!("https://github.com/{}", upstream.repo))),
        license: skill.license,
        stars: None,
        last_updated: None,
        source: CatalogSource::GitHubRepo {
            repo: upstream.repo.clone(),
            ref_: Some(upstream.ref_.clone()),
        },
        trust: upstream.trust,
        metadata,
        tags: skill.tags,
    }
}

/// Shannon built-in skills — always available, no upstream fetch needed.
fn builtin_skills() -> Vec<CatalogEntry> {
    let native = |name: &str,
                 description: &str,
                 trigger: &str,
                 tags: &[&str]| {
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("trigger".to_string(), serde_json::json!(trigger));
        CatalogEntry {
            id: format!("native:skill-{name}"),
            kind: AddonKind::Skill,
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
        native(
            "pdf",
            "Read, search, and extract content from PDF documents.",
            "/pdf",
            &["pdf", "native", "documents"],
        ),
        native(
            "git-workflow",
            "Branch, commit, and PR automation helpers.",
            "/git",
            &["git", "native", "vcs"],
        ),
        native(
            "doc-builder",
            "Generate architecture docs, API references, and ADRs from code scans.",
            "/doc",
            &["docs", "native", "automation"],
        ),
        native(
            "test-scaffolder",
            "Scaffold unit/integration tests following the project's existing patterns.",
            "/test",
            &["testing", "native", "automation"],
        ),
        native(
            "refactor",
            "Scope-safe refactors: extract function, inline variable, rename symbol across module.",
            "/refactor",
            &["refactor", "native", "code"],
        ),
        native(
            "debugger",
            "Systematic debugging: bisect, capture state, isolate root cause.",
            "/debug",
            &["debug", "native", "diagnostics"],
        ),
        native(
            "security-review",
            "Audit code for OWASP top 10, credential leaks, injection, and unsafe patterns.",
            "/sec",
            &["security", "native", "audit"],
        ),
        native(
            "perf-profiler",
            "Profile hot paths, suggest algorithmic improvements, benchmark before/after.",
            "/perf",
            &["performance", "native", "profiling"],
        ),
        native(
            "i18n-helper",
            "Extract user-visible strings, enforce locale-file conventions, machine-translate drafts.",
            "/i18n",
            &["i18n", "native", "localization"],
        ),
        native(
            "sql-builder",
            "Build and explain SQL queries with parameter binding and schema-aware completion.",
            "/sql",
            &["sql", "native", "database"],
        ),
    ]
}

fn default_skill_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("shannon").join("skills"))
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
            "skills": [
                {
                    "name": "brainstorming",
                    "description": "Socratic dialogue for requirements discovery.",
                    "version": "1.0.0",
                    "path": "brainstorming/SKILL.md",
                    "trigger": "/brainstorm",
                    "tags": ["discovery", "requirements"]
                },
                {
                    "name": "tdd",
                    "description": "Test-driven development workflow.",
                    "tags": ["testing"]
                }
            ]
        }"#
        .to_string()
    }

    #[tokio::test]
    async fn lists_builtin_skills_without_network() {
        let client = SkillCatalogClient::new(Arc::new(StaticFetch("{}".to_string())));
        let entries = client.list_skills().await.expect("list");
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
        let client = SkillCatalogClient::new(Arc::new(StaticFetch(manifest_json())))
            .with_cache_dir(tmp.path());
        let entries = client.list_skills().await.expect("list");
        // 10 native + 2 skills × 2 upstreams = 14
        assert_eq!(entries.len(), 14);
        let brainstorming = entries
            .iter()
            .find(|e| e.name == "brainstorming")
            .expect("brainstorming");
        assert_eq!(brainstorming.kind, AddonKind::Skill);
        assert!(brainstorming.id.starts_with("gh:"));
        assert_eq!(brainstorming.trust, TrustLevel::Verified);
        let trigger = brainstorming
            .metadata
            .get("trigger")
            .and_then(|v| v.as_str());
        assert_eq!(trigger, Some("/brainstorm"));
    }

    #[tokio::test]
    async fn serves_cache_on_second_call() {
        let tmp = tempfile::tempdir().expect("tmp");
        let http = Arc::new(StaticFetch(manifest_json()));
        let client = SkillCatalogClient::new(http).with_cache_dir(tmp.path());
        let first = client.list_skills().await.expect("list");
        // Replace the HTTP fetcher with one that would return invalid JSON —
        // if cache works, we should still get the same answer.
        let http_bad = Arc::new(StaticFetch("not json".to_string()));
        let client2 = SkillCatalogClient::new(http_bad).with_cache_dir(tmp.path());
        let second = client2.list_skills().await.expect("list");
        assert_eq!(first.len(), second.len());
        // 10 native + 2 × 2 upstream = 14
        assert_eq!(first.len(), 14);
    }

    #[tokio::test]
    async fn skips_upstream_on_fetch_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let http = Arc::new(StaticFetch("not json".to_string()));
        let client = SkillCatalogClient::new(http).with_cache_dir(tmp.path());
        let entries = client.list_skills().await.expect("list");
        // Native only — upstream skipped.
        assert!(entries.iter().all(|e| e.source == CatalogSource::Native));
    }

    #[test]
    fn manifest_entry_parses_optional_fields() {
        let json = r#"{
            "name": "x",
            "description": "y",
            "trigger": "/x"
        }"#;
        let entry: SkillManifestEntry = serde_json::from_str(json).expect("parse");
        assert_eq!(entry.name, "x");
        assert_eq!(entry.trigger.as_deref(), Some("/x"));
        assert!(entry.version.is_none());
    }
}
