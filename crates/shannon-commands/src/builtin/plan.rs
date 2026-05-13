//! /plan command - Read-only code exploration mode
//!
//! Plan mode restricts the model to read-only operations, allowing
//! exploration and analysis without making any changes. Similar to
//! OpenCode's Plan/Build mode switching.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

const PLAN_PROMPT: &str = r##"
You are now in **Plan Mode** — a read-only exploration and planning mode.

**IMPORTANT**: Do NOT modify any files. Only read, search, and analyze code.

## What You Can Do
- Read files and understand code
- Search the codebase with grep/glob
- Analyze architecture and patterns
- Create detailed implementation plans
- Review code for issues
- Explore dependencies and relationships

## What You Cannot Do
- Edit or write files
- Run commands that modify state (git commit, cargo build --release, etc.)
- Create or delete files
- Modify configuration

## Plan Mode Behavior

1. **Explore thoroughly** — Read all relevant files before forming conclusions
2. **Trace execution paths** — Follow function calls, understand data flow
3. **Identify patterns** — Note existing conventions and coding standards
4. **Map dependencies** — Understand what imports what and why
5. **Generate plans** — Produce step-by-step implementation plans with specific file/line references

## Output Format

When analyzing a task:

### Understanding
Brief summary of what you've learned from exploring the code.

### Current Architecture
Key files, structures, and patterns relevant to the task.

### Implementation Plan
Numbered steps with:
- File to modify/create
- Specific changes needed
- Why this approach
- Potential risks

### Questions
Any ambiguities or decisions that need user input.

When the user is ready to implement, they can use `/build` to switch to implementation mode.
"##;

/// Create the /plan command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "plan".to_string(),
            aliases: vec!["explore".to_string(), "analyze".to_string()],
            description: "Read-only exploration and planning mode".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("[what to analyze]".to_string()),
            when_to_use: Some(
                "Explore code and create plans without making changes".to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: false,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Exploring codebase...".to_string(),
        content_length: 3000,
        arg_names: vec!["task".to_string()],
        allowed_tools: vec![
            "Read".to_string(),
            "Grep".to_string(),
            "Glob".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git diff:*)".to_string(),
            "Bash(git show:*)".to_string(),
            "Bash(cargo check:*)".to_string(),
            "Bash(cargo test -- --list:*)".to_string(),
            "Bash(find:*)".to_string(),
            "Bash(ls:*)".to_string(),
            "Bash(cat:*)".to_string(),
            "Bash(head:*)".to_string(),
            "Bash(wc:*)".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(PLAN_PROMPT.to_string()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_command() {
        let cmd = command();
        assert_eq!(cmd.name(), "plan");
        assert!(cmd.aliases().contains(&"explore".to_string()));
    }
}
