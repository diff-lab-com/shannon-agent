//! Unified extensions hub — catalog, installers, aggregator.
//!
//! See `docs/architecture/unified-hub.md` for the ADR. The hub UI calls the
//! installer trait; per-kind adapters handle the actual install mechanics.

pub mod types;
pub mod aggregator;
pub mod installer;
pub mod catalog;
pub mod oauth;
pub mod mcpb;
pub mod mcp_installers;
pub mod skill_catalog;
pub mod skill_installers;

pub use types::{
    AddonKind, CatalogEntry, CatalogSource, ConfirmationLevel, InstallTarget, InstalledAddon,
    ProgressSink, ProgressEvent, TrustLevel,
};
pub use aggregator::{aggregate_installed, InstalledAddonSummary};
pub use installer::{AddonInstaller, InstallError};
pub use catalog::{
    featured_vendors, FeaturedCategory, FeaturedInstallKind, FeaturedVendor, HttpFetch,
    McpRegistryClient, RegistryResponse, RegistryServer, ReqwestFetch, StaticFetch,
};
pub use mcpb::{extract_mcpb, McpbManifest, McpbServer};
pub use oauth::{OAuthError, PkceContext};
pub use mcp_installers::{
    remove_mcp_server_config, write_mcp_server_config, McpbInstaller, OAuthRemoteMcpInstaller,
    ResolvedMcpInstaller, StdioMcpInstaller, StdioMcpSpec,
};
pub use skill_catalog::{
    skill_upstreams, SkillCatalogClient, SkillManifest, SkillManifestEntry, SkillUpstream,
};
pub use skill_installers::{
    is_skill_installed, list_installed_skills, MarketplacePluginInstaller,
    remove_installed_skill, SkillMarkdownInstaller, InstalledSkill,
};
