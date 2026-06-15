//! Unified extensions hub — catalog schema, installer trait, aggregation.
//!
//! P1 scope: type definitions + installer trait skeleton + aggregator that
//! reads existing local configs (MCP servers, skills, agents). No remote
//! catalog fetch yet (P2+).

pub mod types;
pub mod aggregator;
pub mod installer;

pub use types::{
    AddonKind, CatalogEntry, CatalogSource, InstalledAddon, InstallTarget,
    ConfirmationLevel, ProgressSink, ProgressEvent, TrustLevel,
};
pub use aggregator::{aggregate_installed, InstalledAddonSummary};
pub use installer::{AddonInstaller, InstallError};
