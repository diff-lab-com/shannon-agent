//! /batch command — Parallel worktree-isolated PR creation
//!
//! Decomposes work into independent tasks, runs each in a git worktree,
//! and creates a PR per task. Similar to Claude Code's `/batch` flow.

use crate::command::{Command, CommandBase, CommandSource, PromptCommand, ExecutionContext, CommandAvailability};

/// Create the /batch command
pub fn command() -> Command {
    Command::Prompt(Box::new(PromptCommand {
        base: CommandBase {
            name: "batch".to_string(),
            aliases: vec!["parallel".to_string()],
            description: "Run parallel tasks in isolated worktrees with PR creation".to_string(),
            has_user_specified_description: false,
            availability: vec![CommandAvailability::All],
            source: CommandSource::Builtin,
            is_enabled: true,
            is_hidden: false,
            argument_hint: Some("<task description or file with task list>".to_string()),
            when_to_use: Some(
                "Use when you need to implement multiple independent changes in parallel, \
                 each in its own branch/PR. Provide a description of all tasks or a file path."
                    .to_string(),
            ),
            version: Some("0.1.0".to_string()),
            disable_model_invocation: false,
            user_invocable: true,
            is_workflow: true,
            immediate: false,
            is_sensitive: false,
            user_facing_name: None,
        },
        progress_message: "Running batch tasks in parallel...".to_string(),
        content_length: 6000,
        arg_names: vec!["tasks".to_string()],
        allowed_tools: vec![
            // Agent spawning for parallel execution
            "Agent".to_string(),
            // Worktree management
            "Bash(git worktree:*)".to_string(),
            "Bash(git branch:*)".to_string(),
            "Bash(git checkout:*)".to_string(),
            "Bash(git add:*)".to_string(),
            "Bash(git commit:*)".to_string(),
            "Bash(git push:*)".to_string(),
            "Bash(git status:*)".to_string(),
            "Bash(git diff:*)".to_string(),
            "Bash(git log:*)".to_string(),
            "Bash(git stash:*)".to_string(),
            // PR creation
            "Bash(gh pr create:*)".to_string(),
            "Bash(gh pr view:*)".to_string(),
            "Bash(gh pr list:*)".to_string(),
            // Read operations for context
            "Read".to_string(),
            "Glob".to_string(),
            "Grep".to_string(),
        ],
        model: None,
        hooks: std::collections::HashMap::new(),
        context: ExecutionContext::Inline,
        agent: None,
        paths: vec![],
        prompt_template: Some(BATCH_PROMPT.to_string()),
    }))
}

/// Prompt template for /batch command
const BATCH_PROMPT: &str = r##"## Batch Execution Mode

You are running in **batch mode** — your job is to execute multiple independent tasks in parallel,
each in its own git worktree, and create a PR for each.

## Context

- Current git status: !`git status --short`
- Current branch: !`git branch --show-current`
- Default branch: !`git remote show origin | head -5`
- Existing worktrees: !`git worktree list`
- User's tasks: {tasks}

## Step 1: Decompose Tasks

Analyze the user's request and break it into **independent, parallelizable tasks**. Each task must:
- Be self-contained (no dependencies on other tasks)
- Have a clear scope (specific files/modules to modify)
- Be completable without knowing the results of other tasks

If the tasks are already listed, validate they are independent. If not independent, restructure them.

## Step 2: Create Worktrees

For each task, create a git worktree:

```bash
git worktree add -b <task-slug> .claude/worktrees/<task-slug> HEAD
```

Use descriptive branch names like `batch/fix-auth-validation` or `batch/add-rate-limiting`.

## Step 3: Execute Tasks in Parallel

For each task, use the Agent tool to spawn a sub-agent:

```json
{
  "operation": "spawn",
  "agent_type": "general-purpose",
  "task": "<detailed task description with file paths>",
  "model": null,
  "allowed_tools": ["Read", "Glob", "Grep", "Edit", "Write", "Bash(cargo:*)", "Bash(git add:*)", "Bash(git commit:*)"],
  "context": {
    "working_directory": "<worktree-path>"
  }
}
```

**Important**: Set `context.working_directory` to the worktree path so each agent works in isolation.

## Step 4: Create PRs

After each agent completes successfully:

```bash
cd <worktree-path> && git add -A && git commit -m "<message>"
git push -u origin <branch-name>
gh pr create --title "<title>" --body "<description>"
```

## Step 5: Cleanup

After all tasks complete:
- Report results (success/failure per task, PR URLs)
- Clean up worktrees: `git worktree remove <path>`
- Summarize what was accomplished

## Safety Rules

- **Never** work directly in the main working directory — always use worktrees
- **Never** force push or modify existing branches
- **Never** create more than 10 parallel tasks without user confirmation
- **Always** validate the repo is clean before starting
- If a task fails, report the failure but continue with remaining tasks
- Each PR title should start with a conventional commit prefix (feat/fix/refactor/test/docs)

## Output Format

After all tasks complete, provide a summary table:

| Task | Branch | Status | PR |
|------|--------|--------|----|
| <name> | <branch> | ✅/❌ | <url> |

"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_command_structure() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert_eq!(pc.base.name, "batch");
                assert!(pc.base.aliases.contains(&"parallel".to_string()));
                assert!(pc.base.is_workflow);
                assert!(pc.base.user_invocable);
                assert!(pc.prompt_template.is_some());
                assert!(!pc.allowed_tools.is_empty());
            }
            _ => panic!("Expected Prompt command"),
        }
    }

    #[test]
    fn test_batch_prompt_contains_key_sections() {
        let template = BATCH_PROMPT;
        assert!(template.contains("Step 1: Decompose Tasks"));
        assert!(template.contains("Step 2: Create Worktrees"));
        assert!(template.contains("Step 3: Execute Tasks in Parallel"));
        assert!(template.contains("Step 4: Create PRs"));
        assert!(template.contains("Step 5: Cleanup"));
        assert!(template.contains("git worktree add"));
        assert!(template.contains("gh pr create"));
        assert!(template.contains("Agent"));
    }

    #[test]
    fn test_batch_allowed_tools_include_agent() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert!(pc.allowed_tools.iter().any(|t| t == "Agent"));
                assert!(pc.allowed_tools.iter().any(|t| t.starts_with("Bash(git worktree")));
                assert!(pc.allowed_tools.iter().any(|t| t.starts_with("Bash(gh pr")));
                assert!(pc.allowed_tools.contains(&"Read".to_string()));
            }
            _ => panic!("Expected Prompt command"),
        }
    }

    #[test]
    fn test_batch_has_parallel_alias() {
        let cmd = command();
        match cmd {
            Command::Prompt(pc) => {
                assert_eq!(pc.base.aliases, vec!["parallel"]);
            }
            _ => panic!("Expected Prompt command"),
        }
    }
}
