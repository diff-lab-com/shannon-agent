//! Skill registry for managing available skills

use crate::definition::{Skill, SkillId, SkillSource};
use crate::error::{SkillError, SkillResult};
use crate::loader::load_skill_from_file;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, warn};

/// Registry for managing all available skills
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug)]
struct RegistryInner {
    /// All registered skills by ID
    skills: HashMap<SkillId, Skill>,
    /// Skills indexed by name (for lookup)
    by_name: HashMap<String, SkillId>,
    /// Skills indexed by alias
    by_alias: HashMap<String, SkillId>,
    /// Conditional skills (require path match)
    conditional_skills: HashMap<SkillId, Skill>,
    /// Skills that have been activated
    activated_skills: HashSet<SkillId>,
    /// Dynamic skills discovered during session
    dynamic_skills: HashMap<SkillId, Skill>,
}

impl Default for RegistryInner {
    fn default() -> Self {
        Self {
            skills: HashMap::new(),
            by_name: HashMap::new(),
            by_alias: HashMap::new(),
            conditional_skills: HashMap::new(),
            activated_skills: HashSet::new(),
            dynamic_skills: HashMap::new(),
        }
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SkillRegistry {
    /// Create a new empty skill registry
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(RegistryInner::default())),
        }
    }

    /// Register a new skill
    pub fn register(&self, skill: Skill) -> SkillResult<()> {
        let mut inner = self.inner.write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: skill.name.clone(),
                message: format!("Failed to acquire write lock: {}", e),
            })?;

        // Check for duplicates
        if let Some(existing_id) = inner.by_name.get(&skill.name) {
            if existing_id != &skill.id {
                warn!("Skill name conflict: '{}' ({} vs {})", skill.name, existing_id, skill.id);
            }
        }

        let skill_name = skill.name.clone();
        let skill_id = skill.id.clone();

        // Register by name
        inner.by_name.insert(skill_name.clone(), skill_id.clone());

        // Register aliases
        for alias in &skill.aliases {
            inner.by_alias.insert(alias.clone(), skill_id.clone());
        }

        // Store based on whether it's conditional
        if skill.is_conditional() {
            inner.conditional_skills.insert(skill_id.clone(), skill.clone());
        }

        inner.skills.insert(skill_id.clone(), skill);
        debug!("Registered skill: {} ({})", skill_name, skill_id);

        Ok(())
    }

    /// Register multiple skills
    pub fn register_all(&self, skills: Vec<Skill>) -> SkillResult<()> {
        for skill in skills {
            self.register(skill)?;
        }
        Ok(())
    }

    /// Get a skill by ID
    pub fn get(&self, id: &SkillId) -> SkillResult<Skill> {
        let inner = self.inner.read()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire read lock: {}", e),
            })?;

        inner.skills.get(id)
            .cloned()
            .ok_or_else(|| SkillError::NotFound(id.clone()))
    }

    /// Get a skill by name or alias
    pub fn get_by_name(&self, name: &str) -> SkillResult<Skill> {
        let inner = self.inner.read()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire read lock: {}", e),
            })?;

        // Try direct name first
        if let Some(id) = inner.by_name.get(name) {
            return inner.skills.get(id)
                .cloned()
                .ok_or_else(|| SkillError::NotFound(name.to_string()));
        }

        // Try aliases
        if let Some(id) = inner.by_alias.get(name) {
            return inner.skills.get(id)
                .cloned()
                .ok_or_else(|| SkillError::NotFound(name.to_string()));
        }

        Err(SkillError::NotFound(name.to_string()))
    }

    /// List all skills
    pub fn list(&self) -> Vec<Skill> {
        self.inner.read()
            .map(|inner| inner.skills.values().cloned().collect())
            .unwrap_or_default()
    }

    /// List only user-invocable skills
    pub fn list_user_invocable(&self) -> Vec<Skill> {
        self.list()
            .into_iter()
            .filter(|s| s.is_user_invocable())
            .collect()
    }

    /// List conditional skills that haven't been activated yet
    pub fn list_conditional(&self) -> Vec<Skill> {
        self.inner.read()
            .map(|inner| {
                inner.conditional_skills.values()
                    .filter(|s| !inner.activated_skills.contains(&s.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List dynamically discovered skills
    pub fn list_dynamic(&self) -> Vec<Skill> {
        self.inner.read()
            .map(|inner| inner.dynamic_skills.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a skill exists
    pub fn contains(&self, id: &SkillId) -> bool {
        self.inner.read()
            .map(|inner| inner.skills.contains_key(id))
            .unwrap_or(false)
    }

    /// Remove a skill from the registry
    pub fn remove(&self, id: &SkillId) -> SkillResult<()> {
        let mut inner = self.inner.write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire write lock: {}", e),
            })?;

        if let Some(skill) = inner.skills.remove(id) {
            inner.by_name.remove(&skill.name);
            for alias in &skill.aliases {
                inner.by_alias.remove(alias);
            }
            inner.conditional_skills.remove(id);
            inner.dynamic_skills.remove(id);
            debug!("Removed skill: {} ({})", skill.name, id);
        }

        Ok(())
    }

    /// Clear all skills from the registry
    pub fn clear(&self) -> SkillResult<()> {
        let mut inner = self.inner.write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire write lock: {}", e),
            })?;

        let count = inner.skills.len();
        inner.skills.clear();
        inner.by_name.clear();
        inner.by_alias.clear();
        inner.conditional_skills.clear();
        inner.activated_skills.clear();
        inner.dynamic_skills.clear();

        info!("Cleared {} skills from registry", count);
        Ok(())
    }

    /// Get the total number of registered skills
    pub fn len(&self) -> usize {
        self.inner.read()
            .map(|inner| inner.skills.len())
            .unwrap_or(0)
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Activate conditional skills that match the given paths
    pub fn activate_for_paths(&self, paths: &[String]) -> Vec<SkillId> {
        let mut result = Vec::new();

        let mut inner = match self.inner.write() {
            Ok(guard) => guard,
            Err(_) => return result,
        };

        // Collect skills to activate first, to avoid holding borrow while modifying
        let to_activate: Vec<_> = inner.conditional_skills
            .iter()
            .filter(|(id, _)| !inner.activated_skills.contains(*id))
            .filter_map(|(id, skill)| {
                for path in paths {
                    if skill.matches_path(path) {
                        return Some((id.clone(), skill.clone()));
                    }
                }
                None
            })
            .collect();

        for (id, skill) in to_activate {
            inner.dynamic_skills.insert(id.clone(), skill.clone());
            inner.activated_skills.insert(id.clone());
            result.push(id);
            debug!("Activated conditional skill '{}' for paths: {:?}", skill.name, paths);
        }

        result
    }

    /// Load and register a skill from a file
    pub fn load_from_file(&self, path: &Path) -> SkillResult<Skill> {
        let skill = load_skill_from_file(path)?;
        self.register(skill.clone())?;
        Ok(skill)
    }

    /// Resolve a skill name/alias to its ID
    pub fn resolve_id(&self, name: &str) -> SkillResult<SkillId> {
        let inner = self.inner.read()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire read lock: {}", e),
            })?;

        inner.by_name.get(name)
            .or_else(|| inner.by_alias.get(name))
            .cloned()
            .ok_or_else(|| SkillError::NotFound(name.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_register() {
        let registry = SkillRegistry::new();
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );

        registry.register(skill.clone()).unwrap();
        assert_eq!(registry.len(), 1);
        assert!(registry.contains(&skill.id));
    }

    #[test]
    fn test_registry_get_by_name() {
        let registry = SkillRegistry::new();
        let skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );

        registry.register(skill).unwrap();
        let retrieved = registry.get_by_name("Test").unwrap();
        assert_eq!(retrieved.name, "Test");
    }

    #[test]
    fn test_registry_alias() {
        let registry = SkillRegistry::new();
        let mut skill = Skill::new(
            "test".to_string(),
            "Test".to_string(),
            "A test".to_string(),
            "Content".to_string(),
        );
        skill.aliases = vec!["t".to_string(), "testy".to_string()];

        registry.register(skill).unwrap();
        assert!(registry.get_by_name("t").is_ok());
        assert!(registry.get_by_name("testy").is_ok());
    }

    #[test]
    fn test_conditional_activation() {
        let registry = SkillRegistry::new();
        let mut skill = Skill::new(
            "conditional".to_string(),
            "Conditional".to_string(),
            "A conditional skill".to_string(),
            "Content".to_string(),
        );
        skill.paths = Some(vec!["src".to_string()]); // Simple path pattern
        skill.source = SkillSource::User;

        registry.register(skill).unwrap();
        assert_eq!(registry.list_conditional().len(), 1);

        let activated = registry.activate_for_paths(&["src/main.rs".to_string()]);
        assert_eq!(activated.len(), 1);
        // After activation, the skill is moved to dynamic skills
        assert_eq!(registry.list_conditional().len(), 0);
    }
}
