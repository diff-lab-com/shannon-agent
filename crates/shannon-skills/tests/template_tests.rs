//! Integration tests for the skill template system
//!
//! Tests:
//! - Loading templates from files on disk
//! - Variable substitution with {{var}} placeholders
//! - Rendering with missing/partial variables
//! - Template with no variables (static content)
//! - Round-trip serialization (JSON)
//! - Edge cases: empty template body, duplicate variables

use shannon_skills::SkillTemplate;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[test]
fn test_load_template_from_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("review.md");
    fs::write(
        &file_path,
        r#"---
name: code-review
description: Review code for quality
trigger: /review
---
Review the following code focusing on {{focus}}.

Files: {{files}}
Please provide specific feedback."#,
    )
    .unwrap();

    let template = SkillTemplate::from_file(&file_path).unwrap();
    assert_eq!(template.name, "code-review");
    assert_eq!(template.description, "Review code for quality");
    assert_eq!(template.trigger, "/review");
    assert_eq!(template.variables, vec!["focus", "files"]);
}

#[test]
fn test_render_with_all_variables() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: greet
description: Greeting
trigger: /greet
---
Hello {{name}}, your role is {{role}}."#,
    )
    .unwrap();

    let mut vars = HashMap::new();
    vars.insert("name".to_string(), "Alice".to_string());
    vars.insert("role".to_string(), "reviewer".to_string());

    let rendered = template.render(&vars);
    assert_eq!(rendered, "Hello Alice, your role is reviewer.");
}

#[test]
fn test_render_with_missing_variables_replaces_with_empty() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: partial
description: Partial render
trigger: /partial
---
{{greeting}} {{name}}, welcome to {{place}}!"#,
    )
    .unwrap();

    // Only provide one of three variables
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), "Bob".to_string());

    let rendered = template.render(&vars);
    assert_eq!(rendered, " Bob, welcome to !");
}

#[test]
fn test_render_template_with_no_variables() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: static
description: Static content
trigger: /static
---
This template has no variables at all.
Just plain text content."#,
    )
    .unwrap();

    assert!(template.variables.is_empty());

    let vars = HashMap::new();
    let rendered = template.render(&vars);
    assert_eq!(
        rendered,
        "This template has no variables at all.\nJust plain text content."
    );
}

#[test]
fn test_render_duplicate_variables_only_substituted_once_each() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: repeat
description: Repeated var
trigger: /repeat
---
{{item}} and {{item}} again."#,
    )
    .unwrap();

    // Only one variable despite being used twice
    assert_eq!(template.variables, vec!["item"]);

    let mut vars = HashMap::new();
    vars.insert("item".to_string(), "apple".to_string());

    let rendered = template.render(&vars);
    assert_eq!(rendered, "apple and apple again.");
}

#[test]
fn test_template_from_nonexistent_file() {
    let result = SkillTemplate::from_file(Path::new("/nonexistent/template.md"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Failed to read"));
}

#[test]
fn test_template_from_file_with_invalid_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("bad.md");
    fs::write(&file_path, "No frontmatter at all, just plain text.").unwrap();

    let result = SkillTemplate::from_file(&file_path);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Expected frontmatter"));
}

#[test]
fn test_template_serialization_roundtrip() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: serde
description: Serialization test
trigger: /serde
---
Hello {{who}}, {{action}}!"#,
    )
    .unwrap();

    let json = serde_json::to_string(&template).unwrap();
    let restored: SkillTemplate = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.name, template.name);
    assert_eq!(restored.description, template.description);
    assert_eq!(restored.trigger, template.trigger);
    assert_eq!(restored.variables, template.variables);
    assert_eq!(restored.template, template.template);

    // Verify the restored template still renders correctly
    let mut vars = HashMap::new();
    vars.insert("who".to_string(), "world".to_string());
    vars.insert("action".to_string(), "greet".to_string());
    assert_eq!(restored.render(&vars), "Hello world, greet!");
}

#[test]
fn test_template_render_preserves_non_variable_braces() {
    // Single braces should not be treated as variables
    let template = SkillTemplate::from_markdown(
        r#"---
name: braces
description: Test braces
trigger: /braces
---
Use ${variable} for bash, {{target}} for templates."#,
    )
    .unwrap();

    assert_eq!(template.variables, vec!["target"]);

    let mut vars = HashMap::new();
    vars.insert("target".to_string(), "substitution".to_string());

    let rendered = template.render(&vars);
    assert_eq!(rendered, "Use ${variable} for bash, substitution for templates.");
}

#[test]
fn test_template_with_multiline_content() {
    let template = SkillTemplate::from_markdown(
        r#"---
name: multi
description: Multiline
trigger: /multi
---
Line 1: {{a}}

Line 3: {{b}}

Line 5: end"#,
    )
    .unwrap();

    let mut vars = HashMap::new();
    vars.insert("a".to_string(), "alpha".to_string());
    vars.insert("b".to_string(), "beta".to_string());

    let rendered = template.render(&vars);
    assert!(rendered.starts_with("Line 1: alpha"));
    assert!(rendered.contains("Line 3: beta"));
    assert!(rendered.ends_with("Line 5: end"));
}
