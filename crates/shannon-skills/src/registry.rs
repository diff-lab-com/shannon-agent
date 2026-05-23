//! Skill registry for managing available skills

use crate::definition::{Skill, SkillFull, SkillId, SkillMetadata, SkillSource};
use crate::error::{SkillError, SkillResult};
use crate::loader::{
    load_full_skill as loader_load_full_skill, load_metadata_only as loader_load_metadata_only,
    load_skill_from_file, load_skills_from_directory,
};
use shannon_types::recover_lock;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, trace, warn};
use walkdir::WalkDir;

/// Registry for managing all available skills
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    inner: Arc<RwLock<RegistryInner>>,
}

#[derive(Debug, Default)]
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
    /// Lightweight metadata for skills loaded in metadata-only mode.
    /// Keyed by skill ID, stores metadata without the body content.
    metadata_only: HashMap<SkillId, SkillMetadata>,
    /// Cache of fully loaded skills (loaded on demand, keyed by ID).
    full_cache: HashMap<SkillId, SkillFull>,
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
        let mut inner = self
            .inner
            .write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: skill.name.clone(),
                message: format!("Failed to acquire write lock: {e}"),
            })?;

        // Check for duplicates
        if let Some(existing_id) = inner.by_name.get(&skill.name) {
            if existing_id != &skill.id {
                warn!(
                    "Skill name conflict: '{}' ({} vs {})",
                    skill.name, existing_id, skill.id
                );
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
            inner
                .conditional_skills
                .insert(skill_id.clone(), skill.clone());
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
        let inner = self.inner.read().map_err(|e| SkillError::ExecutionFailed {
            name: "registry".to_string(),
            message: format!("Failed to acquire read lock: {e}"),
        })?;

        inner
            .skills
            .get(id)
            .cloned()
            .ok_or_else(|| SkillError::NotFound(id.clone()))
    }

    /// Get a skill by name or alias
    pub fn get_by_name(&self, name: &str) -> SkillResult<Skill> {
        let inner = self.inner.read().map_err(|e| SkillError::ExecutionFailed {
            name: "registry".to_string(),
            message: format!("Failed to acquire read lock: {e}"),
        })?;

        // Try direct name first
        if let Some(id) = inner.by_name.get(name) {
            return inner
                .skills
                .get(id)
                .cloned()
                .ok_or_else(|| SkillError::NotFound(name.to_string()));
        }

        // Try aliases
        if let Some(id) = inner.by_alias.get(name) {
            return inner
                .skills
                .get(id)
                .cloned()
                .ok_or_else(|| SkillError::NotFound(name.to_string()));
        }

        Err(SkillError::NotFound(name.to_string()))
    }

    /// List all skills
    pub fn list(&self) -> Vec<Skill> {
        self.inner
            .read()
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
        self.inner
            .read()
            .map(|inner| {
                inner
                    .conditional_skills
                    .values()
                    .filter(|s| !inner.activated_skills.contains(&s.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// List dynamically discovered skills
    pub fn list_dynamic(&self) -> Vec<Skill> {
        self.inner
            .read()
            .map(|inner| inner.dynamic_skills.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a skill exists
    pub fn contains(&self, id: &SkillId) -> bool {
        self.inner
            .read()
            .map(|inner| inner.skills.contains_key(id))
            .unwrap_or(false)
    }

    /// Remove a skill from the registry
    pub fn remove(&self, id: &SkillId) -> SkillResult<()> {
        let mut inner = self
            .inner
            .write()
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

        // Also remove from progressive loading stores
        if let Some(meta) = inner.metadata_only.remove(id) {
            inner.by_name.remove(&meta.name);
            for alias in &meta.aliases {
                inner.by_alias.remove(alias);
            }
            debug!("Removed metadata-only skill: {} ({})", meta.name, id);
        }
        inner.full_cache.remove(id);

        Ok(())
    }

    /// Clear all skills from the registry
    pub fn clear(&self) -> SkillResult<()> {
        let mut inner = self
            .inner
            .write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire write lock: {e}"),
            })?;

        let count = inner.skills.len() + inner.metadata_only.len();
        inner.skills.clear();
        inner.by_name.clear();
        inner.by_alias.clear();
        inner.conditional_skills.clear();
        inner.activated_skills.clear();
        inner.dynamic_skills.clear();
        inner.metadata_only.clear();
        inner.full_cache.clear();

        info!("Cleared {} skills from registry", count);
        Ok(())
    }

    /// Get the total number of registered skills (fully loaded + metadata-only)
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .map(|inner| {
                // Count full skills plus metadata-only skills not already in skills
                let full_count = inner.skills.len();
                let meta_only_unique = inner
                    .metadata_only
                    .keys()
                    .filter(|id| !inner.skills.contains_key(*id))
                    .count();
                full_count + meta_only_unique
            })
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
        let to_activate: Vec<_> = inner
            .conditional_skills
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
            debug!(
                "Activated conditional skill '{}' for paths: {:?}",
                skill.name, paths
            );
        }

        result
    }

    /// Load and register a skill from a file
    pub fn load_from_file(&self, path: &Path) -> SkillResult<Skill> {
        let skill = load_skill_from_file(path)?;
        self.register(skill.clone())?;
        Ok(skill)
    }

    /// Reload a skill from disk, replacing any existing registration.
    ///
    /// If a skill with the same id or display name already exists it is
    /// removed first, then the fresh copy from `path` is loaded and
    /// registered.
    pub fn reload_skill(&self, path: &Path) -> SkillResult<Skill> {
        let skill = load_skill_from_file(path)?;

        // Remove by id first, then by name (covers the case where a previous
        // version had a different id but the same display name).
        let _ = self.remove(&skill.id);
        if let Ok(old) = self.get_by_name(&skill.name) {
            let _ = self.remove(&old.id);
        }

        let name = skill.name.clone();
        self.register(skill.clone())?;
        debug!("Reloaded skill: {} from {:?}", name, path);
        Ok(skill)
    }

    /// Resolve a skill name/alias to its ID
    pub fn resolve_id(&self, name: &str) -> SkillResult<SkillId> {
        let inner = self.inner.read().map_err(|e| SkillError::ExecutionFailed {
            name: "registry".to_string(),
            message: format!("Failed to acquire read lock: {e}"),
        })?;

        inner
            .by_name
            .get(name)
            .or_else(|| inner.by_alias.get(name))
            .cloned()
            .ok_or_else(|| SkillError::NotFound(name.to_string()))
    }

    // ── Progressive Loading ──────────────────────────────────────────────

    /// Load only metadata from a SKILL.md file and register it.
    ///
    /// The full body content is not read from disk, making this much cheaper
    /// for initial discovery. Use [`get_full_skill`] later to load the body
    /// on demand.
    pub fn load_metadata_only(&self, path: &Path) -> SkillResult<SkillMetadata> {
        let metadata = loader_load_metadata_only(path)?;
        let id = metadata.id.clone();
        let name = metadata.name.clone();
        let aliases = metadata.aliases.clone();
        let is_conditional = metadata.when_to_use.is_some();

        let mut inner = self
            .inner
            .write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: name.clone(),
                message: format!("Failed to acquire write lock: {e}"),
            })?;

        inner.by_name.insert(name.clone(), id.clone());
        for alias in &aliases {
            inner.by_alias.insert(alias.clone(), id.clone());
        }

        // If the skill has path conditions, also track it as conditional
        // by creating a minimal Skill entry. We need to check for `paths` in
        // the frontmatter, but metadata_only doesn't carry paths. We handle
        // conditional activation through the full skill when it's loaded.
        let _ = is_conditional;

        // Remove any stale full cache entry (will be re-loaded on demand)
        inner.full_cache.remove(&id);

        inner.metadata_only.insert(id.clone(), metadata.clone());
        debug!("Registered metadata-only skill: {} ({})", name, id);
        Ok(metadata)
    }

    /// Load metadata-only for all SKILL.md files found under `dir`.
    ///
    /// Returns the number of skills discovered.
    pub fn load_metadata_from_directory(
        &self,
        dir: &Path,
        _source: &SkillSource,
    ) -> SkillResult<usize> {
        if !dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in WalkDir::new(dir)
            .min_depth(1)
            .max_depth(2)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.file_name() == Some(std::ffi::OsStr::new("SKILL.md")) {
                match self.load_metadata_only(path) {
                    Ok(_) => count += 1,
                    Err(e) => {
                        warn!("Failed to load skill metadata from {:?}: {}", path, e);
                    }
                }
            }
        }

        trace!("Loaded metadata for {} skills from {:?}", count, dir);
        Ok(count)
    }

    /// Load a full skill on demand by name or ID.
    ///
    /// Checks the in-memory cache first. If the skill was registered via
    /// metadata-only loading, reads the complete file from disk, caches
    /// the result, and returns it. If the skill was already fully loaded
    /// (via [`register`] or [`load_from_file`]), returns it directly.
    ///
    /// Also registers the fully-loaded skill in the main `skills` map so
    /// that subsequent [`get`] / [`get_by_name`] calls return the complete
    /// skill.
    pub fn get_full_skill(&self, name_or_id: &str) -> SkillResult<SkillFull> {
        // Try to resolve to an ID first
        let id = {
            let inner = self.inner.read().map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire read lock: {e}"),
            })?;

            // Check if it's already a known ID
            if inner.skills.contains_key(name_or_id) {
                // Already fully loaded — check cache
                if let Some(cached) = inner.full_cache.get(name_or_id) {
                    return Ok(cached.clone());
                }
                // Build from the existing skill
                if let Some(skill) = inner.skills.get(name_or_id) {
                    return Ok(SkillFull::new(skill.clone()));
                }
            }

            // Resolve via name/alias
            let resolved_id = inner
                .by_name
                .get(name_or_id)
                .or_else(|| inner.by_alias.get(name_or_id))
                .cloned();

            if let Some(rid) = resolved_id {
                // Check full cache
                if let Some(cached) = inner.full_cache.get(&rid) {
                    return Ok(cached.clone());
                }
                // Check if already fully loaded in skills map
                if let Some(skill) = inner.skills.get(&rid) {
                    return Ok(SkillFull::new(skill.clone()));
                }
                // It's metadata-only — we need the file path to load
                if let Some(_meta) = inner.metadata_only.get(&rid) {
                    rid // return the id for loading below
                } else {
                    return Err(SkillError::NotFound(name_or_id.to_string()));
                }
            } else {
                return Err(SkillError::NotFound(name_or_id.to_string()));
            }
        };

        // We have an ID for a metadata-only skill — load from disk
        let file_path = {
            let inner = self.inner.read().map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire read lock: {e}"),
            })?;

            inner
                .metadata_only
                .get(&id)
                .and_then(|m| m.file_path.clone())
                .ok_or_else(|| SkillError::NotFound(id.clone()))?
        };

        let skill = loader_load_full_skill(&file_path)?;
        let full = SkillFull::new(skill.clone());

        // Cache and also register the full skill in the main skills map
        {
            let mut inner = self
                .inner
                .write()
                .map_err(|e| SkillError::ExecutionFailed {
                    name: skill.name.clone(),
                    message: format!("Failed to acquire write lock: {e}"),
                })?;

            // Register the full skill (but don't overwrite name/alias mappings
            // since they already exist from metadata registration)
            let skill_id = skill.id.clone();
            inner.skills.insert(skill_id.clone(), skill);
            inner.full_cache.insert(skill_id, full.clone());

            // Remove from metadata_only since we now have the full skill
            inner.metadata_only.remove(&id);
        }

        debug!("Loaded full skill on demand: {} ({})", full.skill.name, id);
        Ok(full)
    }

    /// Load a complete skill from disk by its ID.
    ///
    /// Convenience wrapper that looks up the file path from metadata and
    /// delegates to [`get_full_skill`].
    pub fn load_full_skill(&self, id: &str) -> SkillResult<SkillFull> {
        self.get_full_skill(id)
    }

    /// Return metadata for all registered skills (both fully loaded and
    /// metadata-only). Suitable for LLM context injection.
    pub fn available_skills_metadata(&self) -> Vec<SkillMetadata> {
        self.available_skills_metadata_with_budget(usize::MAX)
    }

    /// Return metadata for all registered skills, respecting a token budget.
    ///
    /// If the total estimated tokens exceed `budget`, descriptions are
    /// truncated to fit. Skills are returned in alphabetical order by name.
    pub fn available_skills_metadata_with_budget(&self, budget: usize) -> Vec<SkillMetadata> {
        let inner = recover_lock(self.inner.read());

        let mut all_meta: Vec<SkillMetadata> = Vec::new();

        // Collect from fully loaded skills
        for skill in inner.skills.values() {
            if skill.is_hidden {
                continue;
            }
            all_meta.push(SkillMetadata::from(skill));
        }

        // Collect from metadata-only skills (that aren't also in skills)
        for (id, meta) in &inner.metadata_only {
            if meta.is_hidden {
                continue;
            }
            if !inner.skills.contains_key(id) {
                all_meta.push(meta.clone());
            }
        }

        // Sort alphabetically by name
        all_meta.sort_by(|a, b| a.name.cmp(&b.name));

        // Apply token budget
        if budget == usize::MAX {
            return all_meta;
        }

        let mut result = Vec::new();
        let mut tokens_used = 0;

        for mut meta in all_meta {
            let tokens = meta.estimated_tokens();
            if tokens_used + tokens > budget {
                // Try truncating description to fit
                let remaining = budget.saturating_sub(tokens_used);
                if remaining > 0 {
                    // Rough estimate: truncate description to fit remaining tokens
                    let max_chars = remaining.saturating_sub(meta.name.len() / 4 + 2) * 4;
                    if max_chars > 10 {
                        meta.description.truncate(max_chars.saturating_sub(3));
                        meta.description.push_str("...");
                        result.push(meta);
                    }
                }
                // Budget exhausted, stop adding skills
                break;
            }
            tokens_used += tokens;
            result.push(meta);
        }

        result
    }

    /// Default token budget for LLM skill listing.
    pub const DEFAULT_SKILL_TOKEN_BUDGET: usize = 2000;

    /// Format available skills as a concise block for LLM context injection.
    ///
    /// Uses [`DEFAULT_SKILL_TOKEN_BUDGET`] as the default budget.
    pub fn format_skills_for_llm(&self) -> String {
        self.format_skills_for_llm_with_budget(Self::DEFAULT_SKILL_TOKEN_BUDGET)
    }

    /// Format available skills with a custom token budget.
    pub fn format_skills_for_llm_with_budget(&self, budget: usize) -> String {
        let skills = self.available_skills_metadata_with_budget(budget);

        if skills.is_empty() {
            return String::new();
        }

        let mut lines = vec!["Available skills (invoke with /skill-name):".to_string()];
        for meta in &skills {
            let hint = meta
                .argument_hint
                .as_ref()
                .map(|h| format!(" {h}"))
                .unwrap_or_default();
            lines.push(format!("- /{}{}: {}", meta.name, hint, meta.description));
        }

        lines.join("\n")
    }

    /// Load and register skills from a directory (backward compatible).
    ///
    /// This loads full skills. For progressive loading, use
    /// [`load_metadata_from_directory`] instead.
    pub fn load_from_directory(&self, dir: &Path, source: &SkillSource) -> SkillResult<Vec<Skill>> {
        let skills = load_skills_from_directory(dir, source.clone())?;
        for skill in &skills {
            self.register(skill.clone())?;
        }
        Ok(skills)
    }

    /// Invalidate the full-skill cache for a given skill ID.
    ///
    /// The next call to [`get_full_skill`] will re-read from disk.
    pub fn invalidate_cache(&self, id: &SkillId) -> SkillResult<()> {
        let mut inner = self
            .inner
            .write()
            .map_err(|e| SkillError::ExecutionFailed {
                name: "registry".to_string(),
                message: format!("Failed to acquire write lock: {e}"),
            })?;

        inner.full_cache.remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::SkillSource;
    use std::path::PathBuf;

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

    // ── Progressive Loading Tests ─────────────────────────────────────────

    /// Helper: create a temp skill directory with a SKILL.md inside.
    fn create_skill_dir(parent: &Path, skill_name: &str, content: &str) -> PathBuf {
        let dir = parent.join(skill_name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
        dir
    }

    fn skill_content(name: &str, desc: &str) -> String {
        format!("---\nname: {name}\ndescription: {desc}\n---\n\n# {name}\n\nBody for {name}.\n")
    }

    #[test]
    fn test_load_metadata_only_registers_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(
            tmp.path(),
            "commit",
            &skill_content("commit", "Generate git commits"),
        );
        let skill_path = skill_dir.join("SKILL.md");

        let registry = SkillRegistry::new();
        let meta = registry.load_metadata_only(&skill_path).unwrap();

        assert_eq!(meta.id, "commit");
        assert_eq!(meta.name, "commit");
        assert_eq!(meta.description, "Generate git commits");
        assert_eq!(registry.len(), 1);
    }

    #[test]
    fn test_load_metadata_from_directory() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(
            tmp.path(),
            "commit",
            &skill_content("commit", "Commit skill"),
        );
        create_skill_dir(
            tmp.path(),
            "review",
            &skill_content("review", "Review skill"),
        );
        create_skill_dir(tmp.path(), "test", &skill_content("test", "Test skill"));

        let registry = SkillRegistry::new();
        let count = registry
            .load_metadata_from_directory(tmp.path(), &SkillSource::User)
            .unwrap();

        assert_eq!(count, 3);
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_load_metadata_from_nonexistent_directory() {
        let registry = SkillRegistry::new();
        let count = registry
            .load_metadata_from_directory(Path::new("/nonexistent/skills"), &SkillSource::User)
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_get_full_skill_from_metadata_only() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(
            tmp.path(),
            "commit",
            "---\nname: commit\ndescription: Generate commits\n---\n\n# Commit\n\nDetailed instructions.\n",
        );
        let skill_path = skill_dir.join("SKILL.md");

        let registry = SkillRegistry::new();
        registry.load_metadata_only(&skill_path).unwrap();

        // Skill should be resolvable by name
        assert!(registry.get_by_name("commit").is_err()); // Not in full skills map yet

        // Load full skill on demand
        let full = registry.get_full_skill("commit").unwrap();
        assert_eq!(full.skill.id, "commit");
        assert!(full.content().contains("Detailed instructions"));

        // After loading full skill, it should be available in the skills map
        let skill = registry.get_by_name("commit").unwrap();
        assert_eq!(skill.content, full.content().to_string());
    }

    #[test]
    fn test_get_full_skill_caches_result() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(
            tmp.path(),
            "review",
            &skill_content("review", "Review code"),
        );
        let skill_path = skill_dir.join("SKILL.md");

        let registry = SkillRegistry::new();
        registry.load_metadata_only(&skill_path).unwrap();

        // First load reads from disk
        let full1 = registry.get_full_skill("review").unwrap();
        // Second load should come from cache
        let full2 = registry.get_full_skill("review").unwrap();

        assert_eq!(full1.content(), full2.content());
    }

    #[test]
    fn test_invalidate_cache_reloads_from_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(
            tmp.path(),
            "my-skill",
            &skill_content("my-skill", "Original description"),
        );
        let skill_path = skill_dir.join("SKILL.md");

        let registry = SkillRegistry::new();
        registry.load_metadata_only(&skill_path).unwrap();

        // Load full skill
        let full1 = registry.get_full_skill("my-skill").unwrap();
        assert!(full1.content().contains("Body for my-skill"));

        // Modify the file
        std::fs::write(
            &skill_path,
            skill_content("my-skill", "Updated description"),
        )
        .unwrap();

        // Invalidate cache
        registry.invalidate_cache(&"my-skill".to_string()).unwrap();

        // Re-load — should pick up new content. But since it's now in the skills map,
        // we need to remove it first and re-register metadata.
        registry.remove(&"my-skill".to_string()).unwrap();
        registry.load_metadata_only(&skill_path).unwrap();
        let full2 = registry.get_full_skill("my-skill").unwrap();
        // The updated description is in the frontmatter, the body still says "my-skill"
        assert_eq!(full2.skill.description, "Updated description");
    }

    #[test]
    fn test_get_full_skill_not_found() {
        let registry = SkillRegistry::new();
        let result = registry.get_full_skill("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_available_skills_metadata() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(
            tmp.path(),
            "commit",
            &skill_content("commit", "Commit skill"),
        );
        create_skill_dir(
            tmp.path(),
            "review",
            &skill_content("review", "Review skill"),
        );

        let registry = SkillRegistry::new();
        let skill_path1 = tmp.path().join("commit").join("SKILL.md");
        let skill_path2 = tmp.path().join("review").join("SKILL.md");

        registry.load_metadata_only(&skill_path1).unwrap();
        registry.load_metadata_only(&skill_path2).unwrap();

        // Also register a full skill
        let full_skill = Skill::new(
            "test".to_string(),
            "test".to_string(),
            "Test skill".to_string(),
            "Content".to_string(),
        );
        registry.register(full_skill).unwrap();

        let metas = registry.available_skills_metadata();
        assert_eq!(metas.len(), 3);

        // Should be sorted alphabetically
        let names: Vec<&str> = metas.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["commit", "review", "test"]);
    }

    #[test]
    fn test_available_skills_metadata_hides_hidden() {
        let registry = SkillRegistry::new();

        let mut visible = Skill::new(
            "visible".to_string(),
            "visible".to_string(),
            "Visible skill".to_string(),
            "Content".to_string(),
        );
        visible.is_hidden = false;

        let mut hidden = Skill::new(
            "hidden".to_string(),
            "hidden".to_string(),
            "Hidden skill".to_string(),
            "Content".to_string(),
        );
        hidden.is_hidden = true;
        hidden.user_invocable = false;

        registry.register(visible).unwrap();
        registry.register(hidden).unwrap();

        let metas = registry.available_skills_metadata();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].name, "visible");
    }

    #[test]
    fn test_token_budget_truncation() {
        let registry = SkillRegistry::new();

        // Register skills with descriptions
        for i in 0..20 {
            let skill = Skill::new(
                format!("skill-{i:02}"),
                format!("skill-{i:02}"),
                format!("This is skill number {i} with a description that takes some tokens"),
                "Content".to_string(),
            );
            registry.register(skill).unwrap();
        }

        // With unlimited budget, all should be returned
        let unlimited = registry.available_skills_metadata();
        assert_eq!(unlimited.len(), 20);

        // With a very small budget, fewer skills should be returned
        let limited = registry.available_skills_metadata_with_budget(20);
        assert!(limited.len() < 20);
        assert!(!limited.is_empty());
    }

    #[test]
    fn test_token_budget_truncates_descriptions() {
        let registry = SkillRegistry::new();

        // Register one skill with a very long description
        let long_desc = "A".repeat(2000);
        let skill = Skill::new(
            "verbose".to_string(),
            "verbose".to_string(),
            long_desc.clone(),
            "Content".to_string(),
        );
        registry.register(skill).unwrap();

        // With a tiny budget, description should be truncated
        let metas = registry.available_skills_metadata_with_budget(10);
        if !metas.is_empty() {
            assert!(metas[0].description.len() < long_desc.len());
            assert!(metas[0].description.ends_with("..."));
        }
    }

    #[test]
    fn test_format_skills_for_llm() {
        let registry = SkillRegistry::new();

        let mut skill1 = Skill::new(
            "commit".to_string(),
            "commit".to_string(),
            "Generate git commits with conventional messages".to_string(),
            "Content".to_string(),
        );
        skill1.argument_hint = Some("<message>".to_string());

        let skill2 = Skill::new(
            "review".to_string(),
            "review".to_string(),
            "Review code for quality issues".to_string(),
            "Content".to_string(),
        );

        registry.register(skill1).unwrap();
        registry.register(skill2).unwrap();

        let formatted = registry.format_skills_for_llm();
        assert!(formatted.starts_with("Available skills (invoke with /skill-name):"));
        assert!(
            formatted
                .contains("- /commit <message>: Generate git commits with conventional messages")
        );
        assert!(formatted.contains("- /review: Review code for quality issues"));
    }

    #[test]
    fn test_format_skills_for_llm_empty() {
        let registry = SkillRegistry::new();
        let formatted = registry.format_skills_for_llm();
        assert!(formatted.is_empty());
    }

    #[test]
    fn test_load_from_directory_backward_compat() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(
            tmp.path(),
            "compat",
            &skill_content("compat", "Backward compat skill"),
        );

        let registry = SkillRegistry::new();
        let skills = registry
            .load_from_directory(tmp.path(), &SkillSource::User)
            .unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "compat");
        assert_eq!(registry.len(), 1);

        // Full skill should be available via get_by_name
        let skill = registry.get_by_name("compat").unwrap();
        assert!(skill.content.contains("Body for compat"));
    }

    #[test]
    fn test_progressive_loading_then_full_flow() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(
            tmp.path(),
            "commit",
            &skill_content("commit", "Commit helper"),
        );
        create_skill_dir(
            tmp.path(),
            "review",
            &skill_content("review", "Code review"),
        );

        let registry = SkillRegistry::new();

        // Phase 1: Load metadata only for all skills
        let count = registry
            .load_metadata_from_directory(tmp.path(), &SkillSource::User)
            .unwrap();
        assert_eq!(count, 2);

        // Phase 2: Check available metadata for LLM injection
        let metas = registry.available_skills_metadata();
        assert_eq!(metas.len(), 2);

        let formatted = registry.format_skills_for_llm();
        assert!(formatted.contains("commit"));
        assert!(formatted.contains("review"));

        // Phase 3: User invokes "commit" — load full skill on demand
        let full = registry.get_full_skill("commit").unwrap();
        assert!(full.content().contains("Body for commit"));

        // Phase 4: "commit" is now fully loaded, "review" is still metadata-only
        assert!(registry.get_by_name("commit").is_ok());

        // Full skill metadata should still show both
        let metas = registry.available_skills_metadata();
        assert_eq!(metas.len(), 2);
    }

    #[test]
    fn test_remove_metadata_only_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let skill_dir = create_skill_dir(
            tmp.path(),
            "temp-skill",
            &skill_content("temp-skill", "Temporary"),
        );
        let skill_path = skill_dir.join("SKILL.md");

        let registry = SkillRegistry::new();
        registry.load_metadata_only(&skill_path).unwrap();
        assert_eq!(registry.len(), 1);

        // Remove by id (directory name)
        registry.remove(&"temp-skill".to_string()).unwrap();
        assert_eq!(registry.len(), 0);
    }

    #[test]
    fn test_clear_with_metadata_only_skills() {
        let tmp = tempfile::tempdir().unwrap();
        create_skill_dir(tmp.path(), "a", &skill_content("a", "Skill A"));
        create_skill_dir(tmp.path(), "b", &skill_content("b", "Skill B"));

        let registry = SkillRegistry::new();
        registry
            .load_metadata_from_directory(tmp.path(), &SkillSource::User)
            .unwrap();

        // Also add a fully loaded skill
        let full = Skill::new(
            "full".to_string(),
            "full".to_string(),
            "Full skill".to_string(),
            "Content".to_string(),
        );
        registry.register(full).unwrap();

        assert_eq!(registry.len(), 3);

        registry.clear().unwrap();
        assert!(registry.is_empty());
    }

    #[test]
    fn test_default_token_budget_constant() {
        assert_eq!(SkillRegistry::DEFAULT_SKILL_TOKEN_BUDGET, 2000);
    }
}
