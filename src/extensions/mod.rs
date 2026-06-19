//! Unified extensions hub — catalog, installers, aggregator.
//!
//! See `docs/architecture/unified-hub.md` for the ADR. The hub UI calls the
//! installer trait; per-kind adapters handle the actual install mechanics.

pub mod agent_catalog;
pub mod agent_installers;
pub mod aggregator;
pub mod catalog;
pub mod data_source_catalog;
pub mod data_source_fetchers;
pub mod data_source_installers;
pub mod installer;
pub mod mcp_installers;
pub mod mcpb;
pub mod oauth;
pub mod security;
pub mod skill_catalog;
pub mod skill_installers;
pub mod types;

pub use agent_catalog::{
    AgentCatalogClient, AgentManifest, AgentManifestEntry, AgentUpstream, agent_upstreams,
};
pub use agent_installers::{
    AgentMarkdownInstaller, AgentRepoInstaller, InstalledAgent, is_agent_installed,
    list_installed_agents, remove_installed_agent,
};
pub use aggregator::{InstalledAddonSummary, aggregate_installed};
pub use catalog::{
    FeaturedCategory, FeaturedInstallKind, FeaturedVendor, HttpFetch, McpRegistryClient,
    RegistryResponse, RegistryServer, ReqwestFetch, StaticFetch, featured_vendors,
};
pub use data_source_catalog::{
    DataSourceAdapter, DataSourceField, DataSourceKind, data_source_adapters,
    data_source_catalog_entries,
};
pub use data_source_installers::{
    InstalledDataSource, install_data_source, is_data_source_installed,
    list_installed_data_sources, read_data_source_config, remove_installed_data_source,
};
pub use installer::{AddonInstaller, InstallError};
pub use mcp_installers::{
    McpbInstaller, OAuthRemoteMcpInstaller, ResolvedMcpInstaller, StdioMcpInstaller, StdioMcpSpec,
    remove_mcp_server_config, write_mcp_server_config,
};
pub use mcpb::{McpbManifest, McpbServer, extract_mcpb};
pub use oauth::{OAuthError, PkceContext};
pub use security::{
    CatalogReport, InjectionMatch, InjectionReport, InjectionRisk, ReportStore, SignatureReport,
    SignatureStatus, add_report, fetch_readme_cached, is_reported, load_reports, remove_report,
    scan_prompt_injection, scan_with_readme, verify_signature,
};
pub use skill_catalog::{
    SkillCatalogClient, SkillManifest, SkillManifestEntry, SkillUpstream, skill_upstreams,
};
pub use skill_installers::{
    InstalledSkill, MarketplacePluginInstaller, SkillMarkdownInstaller, is_skill_installed,
    list_installed_skills, remove_installed_skill,
};
pub use types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressEvent, ProgressSink, TrustLevel,
};
