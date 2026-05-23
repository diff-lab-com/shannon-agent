//! Dynamic skill discovery

use crate::definition::{Skill, SkillSource};
use crate::error::SkillResult;
use crate::loader::{discover_skill_directories, load_skills_from_directory};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{debug, info};

/// Dynamic skill discovery system
pub struct SkillDiscovery {
    /// Discovered skill directories (for deduplication)
    discovered_dirs: HashSet<PathBuf>,
    /// Current working directory for discovery
    cwd: PathBuf,
}

impl SkillDiscovery {
    /// Create a new skill discovery instance
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            discovered_dirs: HashSet::new(),
            cwd,
        }
    }

    /// Discover and load skills for the given file paths
    pub fn discover_for_paths(&mut self, file_paths: &[PathBuf]) -> SkillResult<Vec<Skill>> {
        let skill_dirs = discover_skill_directories(file_paths, &self.cwd);

        if skill_dirs.is_empty() {
            return Ok(Vec::new());
        }

        let mut all_skills = Vec::new();

        for skill_dir in &skill_dirs {
            // Skip if already discovered
            if self.discovered_dirs.contains(skill_dir) {
                continue;
            }

            debug!("Discovering skills from: {:?}", skill_dir);

            // Use CommandsDeprecated source for legacy "commands" directories
            let source = if skill_dir.file_name().is_some_and(|name| name == "commands") {
                SkillSource::CommandsDeprecated
            } else {
                SkillSource::Project
            };

            match load_skills_from_directory(skill_dir, source) {
                Ok(skills) => {
                    if !skills.is_empty() {
                        info!("Discovered {} skills from {:?}", skills.len(), skill_dir);
                        all_skills.extend(skills);
                    }
                    self.discovered_dirs.insert(skill_dir.clone());
                }
                Err(e) => {
                    debug!("Failed to load skills from {:?}: {}", skill_dir, e);
                }
            }
        }

        Ok(all_skills)
    }

    /// Reset discovery state
    pub fn reset(&mut self) {
        self.discovered_dirs.clear();
    }

    /// Get the number of discovered directories
    pub fn discovered_count(&self) -> usize {
        self.discovered_dirs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discovery_reset() {
        let mut discovery = SkillDiscovery::new(PathBuf::from("/tmp"));

        // Reset should clear discovered directories
        discovery.reset();
        assert_eq!(discovery.discovered_count(), 0);
    }
}
