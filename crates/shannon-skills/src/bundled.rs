//! Bundled (built-in) skills

use crate::definition::{Skill, SkillId, SkillSource};
use crate::error::SkillResult;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Initialize and register all bundled skills
pub fn init_bundled_skills(registry: &BundledSkills) -> SkillResult<()> {
    // Git commit skill
    registry.register(create_commit_skill()?)?;

    // PR review skill
    registry.register(create_review_pr_skill()?)?;

    // Git diff skill
    registry.register(create_diff_skill()?)?;

    // Git status skill
    registry.register(create_status_skill()?)?;

    // Help skill
    registry.register(create_help_skill()?)?;

    Ok(())
}

/// Create the git commit skill
fn create_commit_skill() -> SkillResult<Skill> {
    Ok(BundledSkillBuilder::new(
        "commit".to_string(),
        "Git Commit".to_string(),
        "AI-powered git commit with intelligent message generation".to_string(),
    )
    .alias("ci".to_string())
    .when_to_use(
        "Use when you want to commit changes with an AI-generated commit message".to_string(),
    )
    .argument_hint("<optional-commit-message>".to_string())
    .allowed_tools(vec![
        "Bash".to_string(),
        "Read".to_string(),
        "Edit".to_string(),
    ])
    .content(
        r#"# Git Commit

You are an AI git commit assistant. Help the user create a meaningful commit.

## Process

1. **Check current git status**
   ```bash
   git status
   ```

2. **Review staged changes**
   ```bash
   git diff --cached
   ```

3. **Review unstaged changes** (if relevant)
   ```bash
   git diff
   ```

4. **Generate commit message** following these conventions:
   - Use imperative mood ("Add feature" not "Added feature")
   - Limit first line to 50 characters
   - Keep body lines under 72 characters
   - Focus on **what** and **why**, not how
   - Reference issues: `#123`, `Closes #456`

5. **Create the commit**:
   ```bash
   git commit -m "<message>"
   ```

## Tips

- If user provided `${0}`, use it as the commit message
- Look at code changes to understand intent
- Group related changes into one commit
- Ask for confirmation before committing

## Example Commands

```bash
# Stage all changes
git add .

# Stage specific files
git add <files>

# Amend last commit (if needed)
git commit --amend
```
"#
        .to_string(),
    )
    .build())
}

/// Create the PR review skill
fn create_review_pr_skill() -> SkillResult<Skill> {
    Ok(BundledSkillBuilder::new(
        "review-pr".to_string(),
        "Review Pull Request".to_string(),
        "Analyze and review pull requests for code quality, logic, and potential issues"
            .to_string(),
    )
    .alias("pr".to_string())
    .when_to_use("Use when reviewing a pull request or code changes".to_string())
    .argument_hint("<pr-url-or-number>".to_string())
    .allowed_tools(vec!["Bash".to_string(), "Read".to_string()])
    .content(
        r#"# Pull Request Review

You are a code reviewer. Analyze the pull request thoroughly.

## Review Process

1. **Fetch PR information** (if URL provided)
   ```bash
   gh pr view ${0}
   gh pr diff ${0}
   ```

2. **Review the code changes**:
   - **Correctness**: Does the code work as intended?
   - **Design**: Is the solution well-architected?
   - **Security**: Are there any vulnerabilities?
   - - **Performance**: Any performance concerns?
   - **Testing**: Is there adequate test coverage?
   - **Documentation**: Is the code well-documented?

3. **Check CI/CD status**:
   ```bash
   gh pr checks ${0}
   ```

4. **Provide structured feedback**:
   - **Strengths**: What looks good
   - **Issues**: Problems to address
   - **Suggestions**: Optional improvements
   - **Questions**: Clarifications needed

## Review Checklist

- [ ] Code follows project style guidelines
- [ ] No obvious bugs or logic errors
- [ ] Error handling is appropriate
- [ ] Edge cases are considered
- [ ] Tests cover new functionality
- [ ] Documentation is updated
- [ ] No sensitive data (API keys, passwords)
- [ ] Performance implications are acceptable

## Commands

```bash
# View PR details
gh pr view ${0}

# View PR diff
gh pr diff ${0}

# View PR comments
gh pr view ${0} --comments

# Checkout PR branch
gh pr checkout ${0}
```
"#
        .to_string(),
    )
    .build())
}

/// Create the git diff skill
fn create_diff_skill() -> SkillResult<Skill> {
    Ok(BundledSkillBuilder::new(
        "diff".to_string(),
        "Git Diff".to_string(),
        "View and analyze git differences between commits, branches, or files".to_string(),
    )
    .alias("changes".to_string())
    .when_to_use("Use when you want to see what changed between revisions".to_string())
    .argument_hint("<revision>...<revision>".to_string())
    .allowed_tools(vec!["Bash".to_string()])
    .content(
        r#"# Git Diff Viewer

You are a git diff analyzer. Help the user understand code changes.

## Common Diff Commands

### Show unstaged changes
```bash
git diff
```

### Show staged changes
```bash
git diff --cached
```

### Show working tree vs HEAD
```bash
git diff HEAD
```

### Compare branches
```bash
git diff main..feature-branch
git diff feature-branch..main
```

### Compare specific commits
```bash
git diff <commit-a> <commit-b>
```

### Diff by file
```bash
git diff -- <file-path>
git diff --cached -- <file-path>
```

### Show commit changes
```bash
git show <commit>
```

## Analysis

When showing diffs, provide:
- **Summary**: Brief overview of changes
- **Files affected**: List of changed files
- **Key changes**: Important modifications
- **Potential issues**: Conflicts, errors, risks

## Useful Options

- `--stat`: Summary of changes instead of full diff
- `--color-words`: Highlight word-level changes
- `--name-only`: Only show filenames
- `--name-status`: Show filenames with change status

## Interpretation

- `A` file: Added (new file)
- `M` file: Modified (changed file)
- `D` file: Deleted (removed file)
- `R` file: Renamed (moved file)
- `C` file: Copied (copied file)
"#
        .to_string(),
    )
    .build())
}

/// Create the git status skill
fn create_status_skill() -> SkillResult<Skill> {
    Ok(BundledSkillBuilder::new(
        "status".to_string(),
        "Git Status".to_string(),
        "Monitor and alert on git repository status, branches, and potential issues".to_string(),
    )
    .alias("st".to_string())
    .when_to_use("Use to check the current state of the repository".to_string())
    .allowed_tools(vec!["Bash".to_string()])
    .content(
        r#"# Git Status Monitor

You are a git status assistant. Provide a clear overview of repository state.

## Status Check

```bash
git status
```

## Analyze

Report on:

### Branch Status
- **Current branch**: Which branch you're on
- **Branch status**: Ahead/behind remote
- **Unmerged branches**: Other branches to consider

### Changes
- **Staged changes**: Ready to commit
- **Unstaged changes**: Modified but not staged
- **Untracked files**: New files not tracked by git

### Alerts
- **Conflicts**: Merge conflicts that need resolution
- **Stash entries**: Stashed changes to review
- **Detached HEAD**: Not on any branch

## Enhanced Information

```bash
# Show branch tracking info
git status -sb

# Show local branches
git branch

# Show all branches (including remote)
git branch -a

# Show stashed changes
git stash list

# Show untracked files
git ls-files --others --exclude-standard
```

## Recommendations

Based on the status, suggest:
- Next steps (commit, pull, push, merge, etc.)
- Actions needed (resolve conflicts, clean up, etc.)
- Potential issues to address

## Common Scenarios

**Clean working directory**: "Everything is committed. Ready for new work."

**Uncommitted changes**: "You have X uncommitted files. Commit or stash before switching branches."

**Ahead of remote**: "You're N commits ahead. Push your changes with: git push"

**Behind remote**: "You're N commits behind. Pull updates with: git pull"

**Diverged branches**: "Your branch and 'origin/main' have diverged. Consider: git pull --rebase"
"#
        .to_string(),
    )
    .build())
}

/// Create the help skill
fn create_help_skill() -> SkillResult<Skill> {
    Ok(BundledSkillBuilder::new(
        "help".to_string(),
        "Help".to_string(),
        "Interactive help and command discovery for Shannon Code".to_string(),
    )
    .alias("?".to_string())
    .alias("h".to_string())
    .when_to_use("Use when you need help with Shannon Code commands or features".to_string())
    .argument_hint("<topic>".to_string())
    .content(
        r#"# Shannon Code Help

You are a helpful assistant for Shannon Code. Provide clear, actionable guidance.

## Available Skills

Use these skills by typing `/skill-name` in the REPL:

| Skill | Description |
|-------|-------------|
| `/commit` | Create git commits with AI-generated messages |
| `/review-pr` | Review pull requests |
| `/diff` | View git differences |
| `/status` | Check git repository status |
| `/help` | Show this help message |

## Git Commands Reference

### Common Operations
```bash
git status          # Show repository status
git add <file>      # Stage files
git commit -m "msg"  # Commit changes
git push            # Push to remote
git pull            # Pull from remote
git log --oneline   # Show commit history
git diff            # Show changes
```

### Branching
```bash
git branch           # List branches
git branch <name>    # Create branch
git checkout -b <name> # Create and switch
git merge <branch>   # Merge branch
git branch -d <name>  # Delete branch
```

### Stashing
```bash
git stash            # Stash changes
git stash pop        # Apply and remove stash
git stash list       # List stashes
```

## Getting More Help

- Type `/help <topic>` for specific help on a topic
- Type `/help git` for git command reference
- Type `/help skills` for skill documentation

## Topics Available

- `git` - Git command reference
- `skills` - Skill system documentation
- `repl` - Terminal UI commands
- `tools` - Available tools and their usage

## Current Session Info

- **Working Directory**: ${CWD}
- **Session ID**: ${CLAUDE_SESSION_ID}

What would you like help with?
"#
        .to_string(),
    )
    .build())
}

/// Registry for bundled skills that ship with the application
pub struct BundledSkills {
    inner: Arc<RwLock<HashMap<SkillId, Skill>>>,
}

impl Default for BundledSkills {
    fn default() -> Self {
        Self::new()
    }
}

impl BundledSkills {
    /// Create a new bundled skills registry
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a bundled skill
    pub fn register(&self, skill: Skill) -> SkillResult<()> {
        let mut inner =
            self.inner
                .write()
                .map_err(|e| crate::error::SkillError::ExecutionFailed {
                    name: "bundled".to_string(),
                    message: format!("Failed to acquire lock: {e}"),
                })?;

        let mut skill = skill;
        skill.source = SkillSource::Bundled;
        inner.insert(skill.id.clone(), skill);
        Ok(())
    }

    /// Get all bundled skills
    pub fn list(&self) -> Vec<Skill> {
        self.inner
            .read()
            .map(|inner| inner.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Clear all bundled skills
    pub fn clear(&self) -> SkillResult<()> {
        let mut inner =
            self.inner
                .write()
                .map_err(|e| crate::error::SkillError::ExecutionFailed {
                    name: "bundled".to_string(),
                    message: format!("Failed to acquire lock: {e}"),
                })?;

        inner.clear();
        Ok(())
    }

    /// Get the number of bundled skills
    pub fn len(&self) -> usize {
        self.inner.read().map(|inner| inner.len()).unwrap_or(0)
    }

    /// Returns `true` if there are no bundled skills.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Builder for defining bundled skills
pub struct BundledSkillBuilder {
    id: SkillId,
    name: String,
    description: String,
    content: String,
    aliases: Vec<String>,
    when_to_use: Option<String>,
    argument_hint: Option<String>,
    allowed_tools: Vec<String>,
    model: Option<String>,
    disable_model_invocation: bool,
    user_invocable: bool,
    agent: Option<String>,
    files: Option<HashMap<String, String>>,
}

impl BundledSkillBuilder {
    /// Create a new bundled skill builder
    pub fn new(id: SkillId, name: String, description: String) -> Self {
        Self {
            id,
            name,
            description,
            content: String::new(),
            aliases: Vec::new(),
            when_to_use: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            disable_model_invocation: false,
            user_invocable: true,
            agent: None,
            files: None,
        }
    }

    /// Set the skill content (markdown prompt)
    pub fn content(mut self, content: String) -> Self {
        self.content = content;
        self
    }

    /// Add an alias for this skill
    pub fn alias(mut self, alias: String) -> Self {
        self.aliases.push(alias);
        self
    }

    /// Set when to use this skill
    pub fn when_to_use(mut self, when: String) -> Self {
        self.when_to_use = Some(when);
        self
    }

    /// Set argument hint
    pub fn argument_hint(mut self, hint: String) -> Self {
        self.argument_hint = Some(hint);
        self
    }

    /// Set allowed tools
    pub fn allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.allowed_tools = tools;
        self
    }

    /// Set model override
    pub fn model(mut self, model: String) -> Self {
        self.model = Some(model);
        self
    }

    /// Set whether model invocation should be disabled
    pub fn disable_model_invocation(mut self, disable: bool) -> Self {
        self.disable_model_invocation = disable;
        self
    }

    /// Set whether users can invoke this skill
    pub fn user_invocable(mut self, invocable: bool) -> Self {
        self.user_invocable = invocable;
        self
    }

    /// Set the agent for this skill
    pub fn agent(mut self, agent: String) -> Self {
        self.agent = Some(agent);
        self
    }

    /// Set reference files for this skill
    pub fn files(mut self, files: HashMap<String, String>) -> Self {
        self.files = Some(files);
        self
    }

    /// Build the skill
    pub fn build(self) -> Skill {
        let content_length = self.content.len();
        Skill {
            id: self.id.clone(),
            name: self.name,
            description: self.description,
            aliases: self.aliases,
            when_to_use: self.when_to_use,
            argument_hint: self.argument_hint,
            allowed_tools: self.allowed_tools,
            model: self.model,
            disable_model_invocation: self.disable_model_invocation,
            user_invocable: self.user_invocable,
            hooks: None,
            context: None,
            agent: self.agent,
            paths: None,
            version: None,
            source: SkillSource::Bundled,
            skill_root: None, // Will be set if files are extracted
            file_path: None,
            content: self.content,
            content_length,
            is_hidden: !self.user_invocable,
            effort: None,
            arguments: None,
            created_at: chrono::Utc::now(),
            updated_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bundled_skill_builder() {
        let skill = BundledSkillBuilder::new(
            "test".to_string(),
            "Test Skill".to_string(),
            "A test".to_string(),
        )
        .content("Content here".to_string())
        .alias("t".to_string())
        .build();

        assert_eq!(skill.id, "test");
        assert_eq!(skill.name, "Test Skill");
        assert_eq!(skill.aliases, vec!["t".to_string()]);
        assert_eq!(skill.content, "Content here");
    }

    #[test]
    fn test_bundled_skills_registry() {
        let registry = BundledSkills::new();

        let skill =
            BundledSkillBuilder::new("test".to_string(), "Test".to_string(), "A test".to_string())
                .content("Content".to_string())
                .build();

        registry.register(skill).unwrap();
        assert_eq!(registry.len(), 1);

        let skills = registry.list();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].id, "test");
    }

    #[test]
    fn test_init_bundled_skills() {
        let registry = BundledSkills::new();
        init_bundled_skills(&registry).unwrap();

        // Should have 5 bundled skills
        assert_eq!(registry.len(), 5);

        let skills = registry.list();
        let ids: Vec<_> = skills.iter().map(|s| s.id.as_str()).collect();

        assert!(ids.contains(&"commit"));
        assert!(ids.contains(&"review-pr"));
        assert!(ids.contains(&"diff"));
        assert!(ids.contains(&"status"));
        assert!(ids.contains(&"help"));
    }

    #[test]
    fn test_commit_skill() {
        let skill = create_commit_skill().unwrap();
        assert_eq!(skill.id, "commit");
        assert_eq!(skill.name, "Git Commit");
        assert!(skill.aliases.iter().any(|a| a == "ci"));
        assert!(skill.allowed_tools.iter().any(|t| t == "Bash"));
    }

    #[test]
    fn test_review_pr_skill() {
        let skill = create_review_pr_skill().unwrap();
        assert_eq!(skill.id, "review-pr");
        assert_eq!(skill.name, "Review Pull Request");
        assert!(skill.aliases.iter().any(|a| a == "pr"));
    }

    #[test]
    fn test_diff_skill() {
        let skill = create_diff_skill().unwrap();
        assert_eq!(skill.id, "diff");
        assert_eq!(skill.name, "Git Diff");
        assert!(skill.aliases.iter().any(|a| a == "changes"));
    }

    #[test]
    fn test_status_skill() {
        let skill = create_status_skill().unwrap();
        assert_eq!(skill.id, "status");
        assert_eq!(skill.name, "Git Status");
        assert!(skill.aliases.iter().any(|a| a == "st"));
    }

    #[test]
    fn test_help_skill() {
        let skill = create_help_skill().unwrap();
        assert_eq!(skill.id, "help");
        assert_eq!(skill.name, "Help");
        assert!(skill.aliases.iter().any(|a| a == "?"));
        assert!(skill.aliases.iter().any(|a| a == "h"));
    }
}
