//! # Shannon Skills
//!
//! Extensible skill system for Shannon Code.

// Suppress lints that conflict with rustfmt formatting or are style preferences.
#![allow(
    clippy::collapsible_if,
    clippy::collapsible_match,
    clippy::derivable_impls
)]
//!
//! ## Overview
//!
//! The skills system provides a framework for defining, loading, and executing
//! reusable prompts and commands. Skills are defined in markdown files with
//! YAML frontmatter for metadata.
//!
//! ## Architecture
//!
//! - [`definition`]: Core skill types and structures
//! - [`frontmatter`]: Frontmatter parsing for skill metadata
//! - [`loader`]: Loading skills from disk
//! - [`registry`]: Central registry for skill management
//! - [`executor`]: Skill execution engine with argument substitution
//! - [`bundled`]: Built-in skills that ship with the application
//! - [`discovery`]: Dynamic skill discovery at runtime
//!
//! ## Usage
//!
//! ```rust,no_run
//! use shannon_skills::{SkillRegistry, SkillExecutor};
//!
//! // Create a registry
//! let registry = SkillRegistry::new();
//!
//! // Load skills from directory
//! let skills = shannon_skills::loader::load_skills_from_directory(
//!     std::path::Path::new(".claude/skills"),
//!     shannon_skills::definition::SkillSource::User,
//! )?;
//!
//! registry.register_all(skills)?;
//!
//! // Execute a skill
//! let executor = SkillExecutor::new();
//! let skill = registry.get_by_name("my-skill")?;
//! let context = shannon_skills::definition::SkillContext {
//!     arguments: vec!["arg1".to_string()],
//!     cwd: std::path::PathBuf::from("."),
//!     session_id: "session-123".to_string(),
//!     effort_level: "medium".to_string(),
//!     permissions: Default::default(),
//! };
//!
//! let result = executor.execute(&skill, &context)?;
//! println!("{}", result.prompt_content);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

pub mod agent_loader;
pub mod bundled;
pub mod definition;
pub mod discovery;
pub mod error;
pub mod executor;
pub mod frontmatter;
pub mod loader;
pub mod registry;
pub mod template;
pub mod watcher;

// Re-export commonly used types
pub use agent_loader::{
    AgentColor, AgentDefinition, AgentEffort, AgentIsolation, AgentModel, AgentPermissionMode,
    discover_agent_directories, load_agents_from_directory, parse_agent_definition,
};
pub use bundled::{BundledSkillBuilder, BundledSkills, init_bundled_skills};
pub use definition::{Skill, SkillContext, SkillId, SkillPermissions, SkillResult, SkillSource};
pub use discovery::SkillDiscovery;
pub use error::{SkillError, SkillResult as Result};
pub use executor::SkillExecutor;
pub use frontmatter::ParsedSkill;
pub use registry::SkillRegistry;
pub use template::SkillTemplate;
pub use watcher::SkillWatcher;

/// Library version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::const_is_empty)]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
