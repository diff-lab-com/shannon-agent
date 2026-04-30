//! Skill template system
//!
//! Loads skill definitions from Markdown files with YAML frontmatter,
//! providing variable substitution via `{{variable}}` placeholders.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A skill template loaded from a Markdown file with YAML frontmatter.
///
/// The Markdown file format is:
///
/// ```markdown
/// ---
/// name: code-review
/// description: Review code for quality issues
/// trigger: /review
/// ---
/// You are reviewing code for quality issues. Focus on:
/// {{focus_areas}}
///
/// Files to review: {{files}}
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTemplate {
    /// Skill name
    pub name: String,
    /// Short description
    pub description: String,
    /// Trigger pattern (e.g., "/review")
    pub trigger: String,
    /// The template body with `{{variable}}` placeholders
    pub template: String,
    /// Variable names extracted from the template
    #[serde(default)]
    pub variables: Vec<String>,
}

impl SkillTemplate {
    /// Load a skill template from a Markdown string with YAML frontmatter.
    ///
    /// The frontmatter is delimited by `---` and must contain at minimum
    /// `name`, `description`, and `trigger` fields.
    pub fn from_markdown(content: &str) -> Result<Self, String> {
        let (frontmatter_str, template) = split_frontmatter(content)?;

        let frontmatter: TemplateFrontmatter =
            serde_yaml::from_str(frontmatter_str).map_err(|e| format!("YAML parse error: {e}"))?;

        let name = frontmatter.name.ok_or("missing required field: name")?;
        let description = frontmatter.description.ok_or("missing required field: description")?;
        let trigger = frontmatter.trigger.ok_or("missing required field: trigger")?;

        let variables = Self::extract_variables(&template);

        Ok(Self {
            name,
            description,
            trigger,
            template,
            variables,
        })
    }

    /// Load a skill template from a file on disk.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        Self::from_markdown(&content)
    }

    /// Render the template with variable substitution.
    ///
    /// All `{{variable}}` placeholders are replaced with the corresponding
    /// values from `vars`. Missing variables are replaced with an empty string.
    pub fn render(&self, vars: &HashMap<String, String>) -> String {
        let mut result = self.template.clone();
        for var_name in &self.variables {
            let placeholder = format!("{{{{{var_name}}}}}");
            let value = vars.get(var_name).map(|s| s.as_str()).unwrap_or("");
            result = result.replace(&placeholder, value);
        }
        result
    }

    /// Extract `{{variable}}` names from a template string.
    ///
    /// Returns variable names in order of first appearance, without duplicates.
    fn extract_variables(template: &str) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut variables = Vec::new();

        let mut remaining = template;
        while let Some(start) = remaining.find("{{") {
            remaining = &remaining[start + 2..];
            if let Some(end) = remaining.find("}}") {
                let var_name = remaining[..end].trim().to_string();
                if !var_name.is_empty() && seen.insert(var_name.clone()) {
                    variables.push(var_name);
                }
                remaining = &remaining[end + 2..];
            } else {
                break;
            }
        }

        variables
    }
}

/// YAML frontmatter for a skill template.
#[derive(Debug, Clone, Deserialize)]
struct TemplateFrontmatter {
    name: Option<String>,
    description: Option<String>,
    trigger: Option<String>,
}

/// Split a Markdown string into frontmatter and body.
///
/// Expects the content to start with `---\n`, ending with a second `---\n`.
fn split_frontmatter(content: &str) -> Result<(&str, String), String> {
    let trimmed = content.trim_start();

    if !trimmed.starts_with("---") {
        return Err("Expected frontmatter starting with ---".to_string());
    }

    // Skip the opening ---
    let after_opening = &trimmed[3..];

    // Find the closing ---
    let closing_pos = after_opening
        .find("\n---")
        .ok_or("Expected closing --- for frontmatter")?;

    let frontmatter = &after_opening[..closing_pos];
    let body = after_opening[closing_pos + 4..].trim_start().to_string();

    Ok((frontmatter, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_markdown_basic() {
        let md = r#"---
name: code-review
description: Review code for quality issues
trigger: /review
---
You are reviewing code for quality issues. Focus on:
{{focus_areas}}

Files to review: {{files}}"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        assert_eq!(template.name, "code-review");
        assert_eq!(template.description, "Review code for quality issues");
        assert_eq!(template.trigger, "/review");
        assert_eq!(template.variables, vec!["focus_areas", "files"]);
        assert!(template.template.contains("{{focus_areas}}"));
        assert!(template.template.contains("{{files}}"));
    }

    #[test]
    fn test_from_markdown_no_variables() {
        let md = r#"---
name: simple
description: A simple skill
trigger: /simple
---
This template has no variables."#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        assert_eq!(template.name, "simple");
        assert!(template.variables.is_empty());
    }

    #[test]
    fn test_from_markdown_missing_name() {
        let md = r#"---
description: Missing name
trigger: /test
---
Body"#;
        let result = SkillTemplate::from_markdown(md);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing required field: name"));
    }

    #[test]
    fn test_from_markdown_missing_description() {
        let md = r#"---
name: test
trigger: /test
---
Body"#;
        let result = SkillTemplate::from_markdown(md);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing required field: description"));
    }

    #[test]
    fn test_from_markdown_missing_trigger() {
        let md = r#"---
name: test
description: Test
---
Body"#;
        let result = SkillTemplate::from_markdown(md);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing required field: trigger"));
    }

    #[test]
    fn test_from_markdown_no_frontmatter() {
        let md = "Just a plain markdown file";
        let result = SkillTemplate::from_markdown(md);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Expected frontmatter"));
    }

    #[test]
    fn test_render_with_variables() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
Hello {{name}}, welcome to {{place}}!"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("place".to_string(), "Wonderland".to_string());

        let rendered = template.render(&vars);
        assert_eq!(rendered, "Hello Alice, welcome to Wonderland!");
    }

    #[test]
    fn test_render_missing_variable_replaced_with_empty() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
Hello {{name}}!"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        let vars = HashMap::new();
        let rendered = template.render(&vars);
        assert_eq!(rendered, "Hello !");
    }

    #[test]
    fn test_render_partial_variables() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
{{greeting}} {{name}}!"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Bob".to_string());

        let rendered = template.render(&vars);
        assert_eq!(rendered, " Bob!");
    }

    #[test]
    fn test_extract_variables_deduplication() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
{{name}} is {{name}} again and {{name}} once more."#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        // Should only contain "name" once
        assert_eq!(template.variables, vec!["name"]);
    }

    #[test]
    fn test_extract_variables_order() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
{{z_var}} {{a_var}} {{m_var}}"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        // Order of first appearance
        assert_eq!(template.variables, vec!["z_var", "a_var", "m_var"]);
    }

    #[test]
    fn test_extract_variables_with_whitespace() {
        let md = r#"---
name: test
description: Test
trigger: /test
---
{{ name }} {{place}}"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        assert_eq!(template.variables, vec!["name", "place"]);
    }

    #[test]
    fn test_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let file_path = tmp.path().join("test-skill.md");
        std::fs::write(
            &file_path,
            r#"---
name: file-skill
description: Loaded from file
trigger: /file
---
File content: {{item}}"#,
        )
        .unwrap();

        let template = SkillTemplate::from_file(&file_path).unwrap();
        assert_eq!(template.name, "file-skill");
        assert_eq!(template.variables, vec!["item"]);
    }

    #[test]
    fn test_from_file_nonexistent() {
        let result = SkillTemplate::from_file(Path::new("/nonexistent/skill.md"));
        assert!(result.is_err());
    }

    #[test]
    fn test_render_multiline_template() {
        let md = r#"---
name: review
description: Code review
trigger: /review
---
You are reviewing code. Focus on:
{{focus_areas}}

Files: {{files}}

Please provide feedback on:
- Code quality
- Test coverage"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        let mut vars = HashMap::new();
        vars.insert("focus_areas".to_string(), "security, performance".to_string());
        vars.insert("files".to_string(), "src/main.rs, src/lib.rs".to_string());

        let rendered = template.render(&vars);
        assert!(rendered.contains("security, performance"));
        assert!(rendered.contains("src/main.rs, src/lib.rs"));
        assert!(rendered.contains("Please provide feedback on:"));
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let md = r#"---
name: serde-test
description: Serialization test
trigger: /serde
---
Hello {{who}}"#;

        let template = SkillTemplate::from_markdown(md).unwrap();
        let json = serde_json::to_string(&template).unwrap();
        let deserialized: SkillTemplate = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.name, "serde-test");
        assert_eq!(deserialized.trigger, "/serde");
        assert_eq!(deserialized.variables, vec!["who"]);
    }
}
