//! Skill registry for managing available skills

use crate::definition::{Skill, SkillId};
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
#[derive(Default)]
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
                message: format!("Failed to acquire write lock: {e}"),
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
                message: format!("Failed to acquire read lock: {e}"),
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
                message: format!("Failed to acquire read lock: {e}"),
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
                message: format!("Failed to acquire write lock: {e}"),
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
                message: format!("Failed to acquire write lock: {e}"),
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
                message: format!("Failed to acquire read lock: {e}"),
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
    use crate::definition::SkillSource;

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

    // ── Integration Tests ────────────────────────────────────────────────

    #[test]
    fn test_concurrent_skill_registration() {
        use std::sync::{Arc, Mutex};
        use std::thread;

        let registry = Arc::new(Mutex::new(SkillRegistry::new()));
        let num_threads = 10;
        let mut handles = Vec::new();

        // Each thread registers a unique skill
        for i in 0..num_threads {
            let registry_clone = registry.clone();
            let handle = thread::spawn(move || {
                let skill = Skill::new(
                    format!("skill_{i}"),
                    format!("Skill{i}"),
                    format!("Description {i}"),
                    format!("Content {i}"),
                );
                registry_clone.lock().unwrap().register(skill)
            });
            handles.push(handle);
        }

        // Wait for all registrations
        for handle in handles {
            assert!(handle.join().unwrap().is_ok());
        }

        // Verify all skills were registered
        let registry = registry.lock().unwrap();
        assert_eq!(registry.len(), num_threads);
    }

    #[test]
    fn test_skill_lifecycle_full_flow() {
        let registry = SkillRegistry::new();

        // Create and register a bundled skill
        let bundled = Skill::new(
            "bundled".to_string(),
            "Bundled".to_string(),
            "A bundled skill".to_string(),
            "Bundled content".to_string(),
        );
        registry.register(bundled).unwrap();

        // Create and register a user skill with path conditions
        let mut user_skill = Skill::new(
            "react".to_string(),
            "React".to_string(),
            "React helper".to_string(),
            "React content".to_string(),
        );
        user_skill.source = SkillSource::User;
        user_skill.paths = Some(vec!["src".to_string()]); // Simple path pattern that works
        user_skill.aliases = vec!["tsx".to_string()];
        registry.register(user_skill).unwrap();

        // Verify skills are registered
        let all_skills = registry.list();
        assert_eq!(all_skills.len(), 2);

        // Activate based on paths - returns SkillIds
        let paths = vec!["src/main.rs".to_string()];
        let activated_ids = registry.activate_for_paths(&paths);
        assert_eq!(activated_ids.len(), 1);

        // Get the activated skill and verify its name
        let activated_skill = registry.get(&activated_ids[0]).unwrap();
        assert_eq!(activated_skill.name, "React");

        // Get by alias
        let by_alias = registry.get_by_name("tsx");
        assert!(by_alias.is_ok());

        // Remove the skill
        registry.remove(&by_alias.unwrap().id).unwrap();
        assert_eq!(registry.len(), 1); // Only bundled remains
    }

    #[test]
    fn test_multiple_path_patterns_activation() {
        let registry = SkillRegistry::new();

        // Create skills with different path patterns
        let mut frontend = Skill::new(
            "frontend".to_string(),
            "Frontend".to_string(),
            "Frontend helper".to_string(),
            "Frontend content".to_string(),
        );
        frontend.source = SkillSource::User;
        frontend.paths = Some(vec!["src/components".to_string()]);

        let mut backend = Skill::new(
            "backend".to_string(),
            "Backend".to_string(),
            "Backend helper".to_string(),
            "Backend content".to_string(),
        );
        backend.source = SkillSource::User;
        backend.paths = Some(vec!["server".to_string()]);

        registry.register(frontend).unwrap();
        registry.register(backend).unwrap();

        // Test frontend paths
        let frontend_paths = vec!["src/components/Button.tsx".to_string()];
        let activated_ids = registry.activate_for_paths(&frontend_paths);
        assert_eq!(activated_ids.len(), 1);
        let activated_skill = registry.get(&activated_ids[0]).unwrap();
        assert_eq!(activated_skill.name, "Frontend");

        // Need to clear and re-register for second test since skills are activated
        let registry2 = SkillRegistry::new();
        let mut backend2 = Skill::new(
            "backend2".to_string(),
            "Backend".to_string(),
            "Backend helper".to_string(),
            "Backend content".to_string(),
        );
        backend2.source = SkillSource::User;
        backend2.paths = Some(vec!["server".to_string()]);
        registry2.register(backend2).unwrap();

        let backend_paths = vec!["server/main.rs".to_string()];
        let activated_ids = registry2.activate_for_paths(&backend_paths);
        assert_eq!(activated_ids.len(), 1);
        let activated_skill = registry2.get(&activated_ids[0]).unwrap();
        assert_eq!(activated_skill.name, "Backend");
    }

    #[test]
    fn test_skill_discovery_and_search() {
        let registry = SkillRegistry::new();

        // Register multiple skills with different characteristics
        let skills = vec![
            ("auth", "Authentication", "Auth helpers"),
            ("db", "Database", "Database operations"),
            ("api", "API", "API client"),
        ];

        for (id, name, desc) in skills {
            let skill = Skill::new(
                id.to_string(),
                name.to_string(),
                desc.to_string(),
                format!("Content for {name}"),
            );
            registry.register(skill).unwrap();
        }

        // List all skills
        let all_skills = registry.list();
        assert_eq!(all_skills.len(), 3);

        // Search by name prefix (simulating discovery)
        let found = registry.get_by_name("API");
        assert!(found.is_ok());
        assert_eq!(found.unwrap().description, "API client");
    }

    #[test]
    fn test_conditional_skill_state_persistence() {
        let registry = SkillRegistry::new();

        // Create a conditional skill
        let mut conditional = Skill::new(
            "conditional".to_string(),
            "Conditional".to_string(),
            "A conditional skill".to_string(),
            "Content".to_string(),
        );
        conditional.source = SkillSource::User;
        conditional.paths = Some(vec!["src".to_string()]);

        registry.register(conditional).unwrap();

        // Before activation, it's in conditional list
        let conditional_skills = registry.list_conditional();
        assert_eq!(conditional_skills.len(), 1);

        // Activate for matching path - returns SkillIds
        let activated_ids = registry.activate_for_paths(&["src/main.rs".to_string()]);
        assert_eq!(activated_ids.len(), 1);

        // After activation, it's no longer in conditional list
        let conditional_skills = registry.list_conditional();
        assert_eq!(conditional_skills.len(), 0);
    }

    #[test]
    fn test_skill_registry_duplicates_and_overwrites() {
        let registry = SkillRegistry::new();

        // Register a skill
        let skill1 = Skill::new(
            "skill1".to_string(),
            "MySkill".to_string(),
            "Original".to_string(),
            "Content 1".to_string(),
        );
        registry.register(skill1).unwrap();

        // Try to register duplicate ID
        let skill2 = Skill::new(
            "skill1".to_string(), // Same ID
            "MySkill".to_string(),
            "Updated".to_string(),
            "Content 2".to_string(),
        );
        // Register allows duplicate IDs but logs a warning
        // Test that the skill is still registered
        let result = registry.register(skill2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_alias_resolution_priority() {
        let registry = SkillRegistry::new();

        // Register a skill with an alias
        let mut skill = Skill::new(
            "test".to_string(),
            "TestSkill".to_string(),
            "Test".to_string(),
            "Content".to_string(),
        );
        skill.aliases = vec!["t".to_string(), "test".to_string()];
        registry.register(skill).unwrap();

        // Should resolve by name
        let by_name = registry.get_by_name("TestSkill");
        assert!(by_name.is_ok());

        // Should resolve by alias
        let by_alias = registry.get_by_name("t");
        assert!(by_alias.is_ok());

        // Both should point to same skill
        assert_eq!(by_name.unwrap().id, by_alias.unwrap().id);
    }

    #[test]
    fn test_empty_registry_operations() {
        let registry = SkillRegistry::new();

        // Empty registry should have no skills
        assert_eq!(registry.len(), 0);

        // Getting from empty registry should fail
        assert!(registry.get_by_name("nonexistent").is_err());

        // Listing should return empty vectors
        assert!(registry.list().is_empty());
        assert!(registry.list_conditional().is_empty());
        assert!(registry.list_dynamic().is_empty());

        // Activation with no paths should return empty
        let activated = registry.activate_for_paths(&["src/main.rs".to_string()]);
        assert!(activated.is_empty());
    }

    #[test]
    fn test_mixed_conditional_and_regular_skills() {
        let registry = SkillRegistry::new();

        // Register regular skill
        let regular = Skill::new(
            "regular".to_string(),
            "Regular".to_string(),
            "Regular skill".to_string(),
            "Content".to_string(),
        );
        registry.register(regular).unwrap();

        // Register conditional skill
        let mut conditional = Skill::new(
            "conditional".to_string(),
            "Conditional".to_string(),
            "Conditional skill".to_string(),
            "Content".to_string(),
        );
        conditional.source = SkillSource::User;
        conditional.paths = Some(vec!["src".to_string()]);
        registry.register(conditional).unwrap();

        // Verify counts
        let conditional_skills = registry.list_conditional();
        assert_eq!(conditional_skills.len(), 1);

        // Total count should be 2
        assert_eq!(registry.len(), 2);
    }

    #[tokio::test]
    async fn test_async_skill_operations() {
        // Test that skill registry operations are compatible with async contexts
        let _registry = SkillRegistry::new();
        let registry = SkillRegistry::new();

        // Simulate async skill registration
        let skill = Skill::new(
            "async_test".to_string(),
            "AsyncTest".to_string(),
            "Async test skill".to_string(),
            "Content".to_string(),
        );

        // In an async context, registration should still work
        let result = tokio::task::spawn_blocking(move || {
            let reg = SkillRegistry::new();
            reg.register(skill)
        })
        .await
        .unwrap();

        assert!(result.is_ok());
    }

    #[test]
    fn test_remove_skill_clears_all_indices() {
        let registry = SkillRegistry::new();

        // Register a skill with aliases
        let mut skill = Skill::new(
            "test".to_string(),
            "TestSkill".to_string(),
            "Test".to_string(),
            "Content".to_string(),
        );
        skill.aliases = vec!["alias1".to_string(), "alias2".to_string()];
        let skill_id = skill.id.clone();
        registry.register(skill).unwrap();

        // Verify it's registered
        assert!(registry.contains(&skill_id));
        assert!(registry.get_by_name("TestSkill").is_ok());
        assert!(registry.get_by_name("alias1").is_ok());

        // Remove the skill
        registry.remove(&skill_id).unwrap();

        // Verify it's gone
        assert!(!registry.contains(&skill_id));
        assert!(registry.get_by_name("TestSkill").is_err());
        assert!(registry.get_by_name("alias1").is_err());
    }

    #[test]
    fn test_clear_registry() {
        let registry = SkillRegistry::new();

        // Register multiple skills
        for i in 0..5 {
            let skill = Skill::new(
                format!("skill_{i}"),
                format!("Skill{i}"),
                format!("Description {i}"),
                "Content".to_string(),
            );
            registry.register(skill).unwrap();
        }

        assert_eq!(registry.len(), 5);

        // Clear all
        registry.clear().unwrap();

        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }
}
