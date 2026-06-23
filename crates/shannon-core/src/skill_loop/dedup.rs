//! Deduplication logic for skill proposals
//!
//! This module provides similarity checking to prevent duplicate skills from being created.

use crate::skill_loop::types::SkillProposal;
use std::collections::HashSet;

/// Calculate Jaccard similarity between two strings using trigrams
///
/// Returns a value between 0.0 (no similarity) and 1.0 (identical).
pub fn jaccard_similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }

    let a_trigrams: HashSet<String> = trigrams(a);
    let b_trigrams: HashSet<String> = trigrams(b);

    if a_trigrams.is_empty() && b_trigrams.is_empty() {
        return 1.0;
    }

    if a_trigrams.is_empty() || b_trigrams.is_empty() {
        return 0.0;
    }

    let intersection: HashSet<_> = a_trigrams.intersection(&b_trigrams).collect();
    let union: HashSet<_> = a_trigrams.union(&b_trigrams).collect();

    intersection.len() as f32 / union.len() as f32
}

/// Generate trigrams from a string
fn trigrams(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.to_lowercase().chars().collect();
    let mut result = HashSet::new();

    for i in 0..chars.len().saturating_sub(2) {
        if i + 3 <= chars.len() {
            let trigram: String = chars[i..i + 3].iter().collect();
            if trigram.chars().all(|c| c.is_alphanumeric()) {
                result.insert(trigram);
            }
        }
    }

    result
}

/// Find the most similar existing skill to a proposal
///
/// Returns the highest similarity score found, or None if no skills exist.
///
/// The similarity is calculated based on:
/// - Name (40% weight)
/// - Description (40% weight)
/// - Trigger patterns (20% weight)
pub fn find_similar_skill(proposal: &SkillProposal, existing_skills: &[String]) -> Option<f32> {
    if existing_skills.is_empty() {
        return None;
    }

    let mut max_similarity = 0.0f32;

    for skill_text in existing_skills {
        // Calculate similarity for each component
        let name_sim = jaccard_similarity(&proposal.name, &extract_name(skill_text));

        let desc_sim = jaccard_similarity(&proposal.description, &extract_description(skill_text));

        let triggers_sim = proposal
            .trigger_patterns
            .iter()
            .map(|pattern| jaccard_similarity(pattern, skill_text))
            .reduce(f32::max)
            .unwrap_or(0.0);

        // Weighted average
        let similarity = (name_sim * 0.4) + (desc_sim * 0.4) + (triggers_sim * 0.2);
        max_similarity = max_similarity.max(similarity);
    }

    Some(max_similarity)
}

/// Extract skill name from TOML content
fn extract_name(toml: &str) -> String {
    toml.lines()
        .find(|line| line.starts_with("name = "))
        .and_then(|line| line.split('"').nth(1).or_else(|| line.split('\'').nth(1)))
        .unwrap_or("")
        .to_string()
}

/// Extract skill description from TOML content
fn extract_description(toml: &str) -> String {
    toml.lines()
        .find(|line| line.starts_with("description = "))
        .and_then(|line| line.split('"').nth(1).or_else(|| line.split('\'').nth(1)))
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jaccard_identical() {
        assert_eq!(jaccard_similarity("test", "test"), 1.0);
        assert_eq!(jaccard_similarity("", ""), 1.0);
    }

    #[test]
    fn test_jaccard_no_similarity() {
        let result = jaccard_similarity("abc", "xyz");
        assert!(result < 0.2); // Very low similarity
    }

    #[test]
    fn test_jaccard_partial_similarity() {
        let result = jaccard_similarity("security review", "security code review");
        assert!(result > 0.3 && result < 0.9);
    }

    #[test]
    fn test_extract_name() {
        let toml = r#"
name = "Test Skill"
description = "A description"
"#;
        assert_eq!(extract_name(toml), "Test Skill");
    }

    #[test]
    fn test_extract_description() {
        let toml = r#"
name = "Test Skill"
description = "A description"
"#;
        assert_eq!(extract_description(toml), "A description");
    }

    #[test]
    fn test_extract_name_empty() {
        let toml = "description = \"No name here\"";
        assert_eq!(extract_name(toml), "");
    }

    #[test]
    fn test_find_similar_skill_empty() {
        let proposal = SkillProposal {
            id: uuid::Uuid::new_v4(),
            name: "Test".to_string(),
            slug: "test".to_string(),
            description: "Test desc".to_string(),
            trigger_patterns: vec![],
            example_workflow: String::new(),
            source_task_id: None,
            created_at: chrono::Utc::now(),
            status: super::super::types::ProposalStatus::Pending,
            suggested_metadata: None,
        };

        let result = find_similar_skill(&proposal, &[]);
        assert!(result.is_none());
    }

    #[test]
    fn test_find_similar_skill_above_threshold() {
        let proposal = SkillProposal {
            id: uuid::Uuid::new_v4(),
            name: "Security Review".to_string(),
            slug: "security-review".to_string(),
            description: "Review for security".to_string(),
            trigger_patterns: vec!["when user asks for security".to_string()],
            example_workflow: String::new(),
            source_task_id: None,
            created_at: chrono::Utc::now(),
            status: super::super::types::ProposalStatus::Pending,
            suggested_metadata: None,
        };

        let existing = vec![
            r#"name = "Security Code Review"
description = "Review for security vulnerabilities""#
                .to_string(),
        ];

        let result = find_similar_skill(&proposal, &existing).unwrap();
        assert!(result > 0.5); // High similarity expected
    }
}
