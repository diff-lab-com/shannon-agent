//! Integration tests for frontmatter parsing
//!
//! Tests:
//! - Full frontmatter with all optional fields
//! - Aliases and argument configurations
//! - Hooks configuration (pre/post sampling)
//! - Execution context (inline/fork)
//! - Effort level parsing
//! - Invalid YAML handling
//! - Empty frontmatter defaults
//! - Shell field parsing

use shannon_skills::frontmatter::{
    ArgumentConfig, EffortLevel, ExecutionContext, parse_skill_frontmatter,
};
use std::str::FromStr;

#[test]
fn test_parse_complete_frontmatter() {
    let content = r#"---
name: commit
description: Generate conventional commits
alias:
  - ci
  - c
when_to_use: when you need to commit changes
argument-hint: "<message>"
allowed-tools:
  - bash
  - read
  - write
model: sonnet
disable-model-invocation: false
user-invocable: true
context: inline
agent: executor
paths:
  - src
  - tests
version: "1.0"
arguments:
  - message
  - files
effort: high
shell: /bin/bash
---

Generate a conventional commit message."#;

    let parsed = parse_skill_frontmatter(content, "commit").unwrap();
    let fm = &parsed.frontmatter;

    assert_eq!(fm.name, Some("commit".to_string()));
    assert_eq!(
        fm.description,
        Some("Generate conventional commits".to_string())
    );
    assert_eq!(fm.aliases, Some(vec!["ci".to_string(), "c".to_string()]));
    assert_eq!(
        fm.when_to_use,
        Some("when you need to commit changes".to_string())
    );
    assert_eq!(fm.argument_hint, Some("<message>".to_string()));
    assert_eq!(
        fm.allowed_tools,
        Some(vec![
            "bash".to_string(),
            "read".to_string(),
            "write".to_string()
        ])
    );
    assert_eq!(fm.model, Some("sonnet".to_string()));
    assert_eq!(fm.disable_model_invocation, Some(false));
    assert_eq!(fm.user_invocable, Some(true));
    assert_eq!(fm.context, Some(ExecutionContext::Inline));
    assert_eq!(fm.agent, Some("executor".to_string()));
    assert_eq!(fm.paths, Some(vec!["src".to_string(), "tests".to_string()]));
    assert_eq!(fm.version, Some("1.0".to_string()));
    assert_eq!(fm.effort, Some(EffortLevel::High));
    assert_eq!(fm.shell, Some("/bin/bash".to_string()));

    // Verify argument config
    let args = fm.arguments.as_ref().unwrap();
    assert_eq!(args.names(), vec!["message", "files"]);

    // Body should be the content after frontmatter
    assert_eq!(parsed.body, "Generate a conventional commit message.");
}

#[test]
fn test_parse_frontmatter_single_argument() {
    let content = r#"---
name: deploy
description: Deploy to production
arguments: environment
---

Deploy to the specified environment."#;

    let parsed = parse_skill_frontmatter(content, "deploy").unwrap();
    let args = parsed.frontmatter.arguments.as_ref().unwrap();

    match args {
        ArgumentConfig::Single(name) => assert_eq!(name, "environment"),
        ArgumentConfig::Multiple(names) => panic!("expected Single, got Multiple: {names:?}"),
    }
    assert_eq!(args.names(), vec!["environment"]);
}

#[test]
fn test_parse_frontmatter_multiple_arguments() {
    let content = r#"---
name: refactor
description: Refactor code
arguments:
  - scope
  - strategy
  - target_files
---

Refactor the codebase."#;

    let parsed = parse_skill_frontmatter(content, "refactor").unwrap();
    let args = parsed.frontmatter.arguments.as_ref().unwrap();

    match args {
        ArgumentConfig::Single(_) => panic!("expected Multiple"),
        ArgumentConfig::Multiple(names) => {
            assert_eq!(names, &vec!["scope", "strategy", "target_files"]);
        }
    }
    assert_eq!(args.names(), vec!["scope", "strategy", "target_files"]);
}

#[test]
fn test_parse_frontmatter_hooks() {
    let content = r#"---
name: hooked
description: Skill with hooks
hooks:
  preSamplingHook:
    - check-prerequisites
    - validate-args
  postSamplingHook:
    - notify-complete
---

Body content."#;

    let parsed = parse_skill_frontmatter(content, "hooked").unwrap();
    let hooks = parsed.frontmatter.hooks.as_ref().unwrap();

    assert_eq!(
        hooks.pre_sampling,
        Some(vec![
            "check-prerequisites".to_string(),
            "validate-args".to_string()
        ])
    );
    assert_eq!(
        hooks.post_sampling,
        Some(vec!["notify-complete".to_string()])
    );
}

#[test]
fn test_parse_frontmatter_execution_context_fork() {
    let content = r#"---
name: isolated
description: Runs in fork
context: fork
---

Forked execution body."#;

    let parsed = parse_skill_frontmatter(content, "isolated").unwrap();
    assert_eq!(parsed.frontmatter.context, Some(ExecutionContext::Fork));
}

#[test]
fn test_parse_frontmatter_empty_frontmatter_defaults() {
    let content = r#"---
---

Just a body with empty frontmatter."#;

    let parsed = parse_skill_frontmatter(content, "empty").unwrap();
    let fm = &parsed.frontmatter;

    // All optional fields should be None/default
    assert!(fm.name.is_none());
    assert!(fm.description.is_none());
    assert!(fm.aliases.is_none());
    assert!(fm.when_to_use.is_none());
    assert!(fm.argument_hint.is_none());
    assert!(fm.allowed_tools.is_none());
    assert!(fm.model.is_none());
    assert!(fm.disable_model_invocation.is_none());
    assert!(fm.user_invocable.is_none());
    assert!(fm.hooks.is_none());
    assert!(fm.context.is_none());
    assert!(fm.agent.is_none());
    assert!(fm.paths.is_none());
    assert!(fm.version.is_none());
    assert!(fm.arguments.is_none());
    assert!(fm.effort.is_none());
    assert!(fm.shell.is_none());
    assert_eq!(parsed.body, "Just a body with empty frontmatter.");
}

#[test]
fn test_parse_frontmatter_invalid_yaml() {
    let content = r#"---
name: [broken yaml
description: missing quote
---

Body."#;

    let result = parse_skill_frontmatter(content, "broken");
    assert!(result.is_err());
    let err_msg = format!("{result:?}");
    // Should be a FrontmatterParse error
    assert!(err_msg.contains("FrontmatterParse") || err_msg.contains("parse"));
}

#[test]
fn test_parse_frontmatter_missing_closing_delimiter() {
    let content = r#"---
name: no-close
description: No closing delimiter

Body without closing ---."#;

    let result = parse_skill_frontmatter(content, "no-close");
    assert!(result.is_err());
}

#[test]
fn test_parse_frontmatter_no_frontmatter_treats_all_as_body() {
    let content = "This is just body text with no frontmatter at all.";

    let parsed = parse_skill_frontmatter(content, "plain").unwrap();
    assert!(parsed.frontmatter.name.is_none());
    assert_eq!(parsed.body, content);
}

#[test]
fn test_parse_frontmatter_preserves_raw_content() {
    let content = r#"---
name: raw-test
description: Verify raw is preserved
---

Body here."#;

    let parsed = parse_skill_frontmatter(content, "raw-test").unwrap();
    assert_eq!(parsed.raw, content);
}

#[test]
fn test_effort_level_from_str() {
    assert_eq!(
        EffortLevel::from_str("minimal").unwrap(),
        EffortLevel::Minimal
    );
    assert_eq!(EffortLevel::from_str("low").unwrap(), EffortLevel::Low);
    assert_eq!(
        EffortLevel::from_str("medium").unwrap(),
        EffortLevel::Medium
    );
    assert_eq!(EffortLevel::from_str("high").unwrap(), EffortLevel::High);
    assert_eq!(
        EffortLevel::from_str("maximum").unwrap(),
        EffortLevel::Maximum
    );
    assert!(EffortLevel::from_str("invalid").is_err());
}

#[test]
fn test_effort_level_case_insensitive() {
    assert_eq!(EffortLevel::from_str("High").unwrap(), EffortLevel::High);
    assert_eq!(EffortLevel::from_str("LOW").unwrap(), EffortLevel::Low);
    assert_eq!(
        EffortLevel::from_str("Medium").unwrap(),
        EffortLevel::Medium
    );
}

#[test]
fn test_effort_level_default_is_medium() {
    assert_eq!(EffortLevel::default(), EffortLevel::Medium);
}

#[test]
fn test_execution_context_equality() {
    assert_eq!(ExecutionContext::Inline, ExecutionContext::Inline);
    assert_eq!(ExecutionContext::Fork, ExecutionContext::Fork);
    assert_ne!(ExecutionContext::Inline, ExecutionContext::Fork);
}

#[test]
fn test_parse_frontmatter_disable_model_invocation_true() {
    let content = r#"---
name: prompt-only
description: Prompt only skill
disable-model-invocation: true
---

Just a prompt template."#;

    let parsed = parse_skill_frontmatter(content, "prompt-only").unwrap();
    assert_eq!(parsed.frontmatter.disable_model_invocation, Some(true));
}

#[test]
fn test_parse_frontmatter_user_invocable_false() {
    let content = r#"---
name: internal
description: Internal skill
user-invocable: false
---

Internal only."#;

    let parsed = parse_skill_frontmatter(content, "internal").unwrap();
    assert_eq!(parsed.frontmatter.user_invocable, Some(false));
}
