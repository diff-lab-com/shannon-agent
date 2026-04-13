//! Integration tests for Skill lifecycle and registry operations
//!
//! Tests:
//! - Skill registration and discovery
//! - Conditional skill activation based on file paths
//! - Concurrent skill operations
//! - Skill state persistence

use shannon_skills::registry::SkillRegistry;
use shannon_skills::definition::Skill;

#[tokio::test]
async fn test_concurrent_skill_registration() {
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

#[tokio::test]
async fn test_skill_lifecycle_full_flow() {
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
    user_skill.paths = Some(vec!["src".to_string()]);
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
    frontend.paths = Some(vec!["src/components".to_string()]);

    let mut backend = Skill::new(
        "backend".to_string(),
        "Backend".to_string(),
        "Backend helper".to_string(),
        "Backend content".to_string(),
    );
    backend.paths = Some(vec!["server".to_string()]);

    registry.register(frontend).unwrap();
    registry.register(backend).unwrap();

    // Test frontend paths
    let frontend_paths = vec!["src/components/Button.tsx".to_string()];
    let activated_ids = registry.activate_for_paths(&frontend_paths);
    assert_eq!(activated_ids.len(), 1);
    let activated_skill = registry.get(&activated_ids[0]).unwrap();
    assert_eq!(activated_skill.name, "Frontend");
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
        "conditional2".to_string(),
        "Conditional".to_string(),
        "Conditional skill".to_string(),
        "Content".to_string(),
    );
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
        "test_remove".to_string(),
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
            format!("skill_clear_{i}"),
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
